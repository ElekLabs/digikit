#!/usr/bin/env bash
# Remove finished lanes. Run through `tools/devtools/lane.sh --prune`.
#
#   tools/devtools/lane.sh --prune [--days N] [--dry-run] [--only NAME]...
#
# A lane is removed only when it is under the lanes directory, clean, merged
# into origin/main, idle for N days (default 7) and unused. Every other lane is
# listed with the reason it was kept. Portable to bash 3.2 (macOS).
set -euo pipefail

days=7
dry=0
only=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --days) days=${2:?--days needs a number}; shift 2 ;;
    --dry-run) dry=1; shift ;;
    --only) only+=("${2:?--only needs a lane name}"); shift 2 ;;
    *) echo "usage: tools/devtools/lane.sh --prune [--days N] [--dry-run] [--only NAME]..." >&2; exit 2 ;;
  esac
done
case $days in '' | *[!0-9]*) echo "error: --days must be a whole number" >&2; exit 2 ;; esac

# Read the whole listing (see tools/devtools/worktree-setup.sh about SIGPIPE).
listing=$(git worktree list --porcelain)
main=$(printf '%s\n' "$listing" | awk '/^worktree / && !n {print $2; n=1}')
lanes_dir="$(dirname "$main")/lanes"
git -C "$main" fetch -q origin main
now=$(date +%s)

mtime() { stat -f %m "$1" 2>/dev/null || stat -c %Y "$1"; }

# Newest of the worktree's gitdir index, HEAD and logs/HEAD, and the lane
# directory's own mtime.
newest_mtime() {
  local lane=$1 gitdir best=0 t f
  gitdir=$(git -C "$lane" rev-parse --absolute-git-dir)
  for f in "$gitdir/index" "$gitdir/HEAD" "$gitdir/logs/HEAD" "$lane"; do
    if [[ -e $f ]]; then
      t=$(mtime "$f")
      if ((t > best)); then best=$t; fi
    fi
  done
  echo "$best"
}

# Print "PID KIND" for processes whose command line or cwd is inside the lane;
# KIND is "daemon" for an Nx daemon of any kind, else "proc".
lane_processes() {
  local lane=$1 psout
  psout=$(ps -Ao pid,command)
  {
    printf '%s\n' "$psout" | awk -v lane="$lane" -v self="$$" '
      NR > 1 && $1 != self {
        i = index($0, lane)
        while (i > 0) {
          c = substr($0, i + length(lane), 1)
          if (c == "" || c == "/" || c == " ") { print $1; break }
          j = index(substr($0, i + 1), lane)
          i = j ? i + j : 0
        }
      }'
    if command -v lsof >/dev/null 2>&1; then
      lsof -a -d cwd -Fpn 2>/dev/null | awk -v lane="$lane" -v self="$$" '
        /^p/ { pid = substr($0, 2) }
        /^n/ { p = substr($0, 2)
               if (pid != self && (p == lane || index(p, lane "/") == 1)) print pid }'
    else
      local d p
      for d in /proc/[0-9]*; do
        p=$(readlink "$d/cwd" 2>/dev/null || true)
        if [[ $p == "$lane" || $p == "$lane"/* ]]; then echo "${d#/proc/}"; fi
      done
    fi
  } | sort -u | while read -r pid; do
    if printf '%s\n' "$psout" | awk -v pid="$pid" '$1 == pid' | grep -Eq 'nx.*(start-daemon|daemon)|daemon.*nx'; then
      echo "$pid daemon"
    else
      echo "$pid proc"
    fi
  done
}

# Why a lane is kept ("" when it may go).
keep_reason() {
  local lane=$1 head newest users idle_days
  if [[ -n $(git --no-optional-locks -C "$lane" status --porcelain) ]]; then echo "uncommitted or untracked files"; return; fi
  head=$(git -C "$lane" rev-parse HEAD)
  if ! git -C "$main" merge-base --is-ancestor "$head" origin/main; then echo "not merged into origin/main"; return; fi
  newest=$(newest_mtime "$lane")
  idle_days=$(((now - newest) / 86400))
  if ((now - newest < days * 86400)); then echo "idle ${idle_days}d, under ${days}d"; return; fi
  users=$(lane_processes "$lane")
  if [[ $dry == 0 && $users == *daemon* && -x $lane/node_modules/.bin/nx ]]; then
    (cd "$lane" && ./node_modules/.bin/nx daemon --stop >/dev/null 2>&1) || true
    sleep 1
    users=$(lane_processes "$lane")
  fi
  if [[ $dry == 1 ]]; then users=$(printf '%s\n' "$users" | grep -v ' daemon$' || true); fi
  if [[ -n $users ]]; then
    echo "in use by pid $(printf '%s\n' "$users" | awk '{printf "%s%s", s, $1; s=","}')"
    return
  fi
}

removed=0
kept=0
seen=" "
while read -r lane; do
  [[ -n $lane ]] || continue
  name=${lane#"$lanes_dir"/}
  # Only direct children of the lanes directory.
  [[ $name != */* ]] || continue
  if [[ ${#only[@]} -gt 0 ]]; then
    hit=0
    for o in "${only[@]}"; do if [[ $o == "$name" ]]; then hit=1; fi; done
    if [[ $hit == 0 ]]; then continue; fi
  fi
  seen="$seen$name "
  if [[ ! -d $lane ]]; then
    echo "kept $name: directory is missing (run git worktree prune)"; kept=$((kept + 1)); continue
  fi
  branch=$(git -C "$lane" symbolic-ref -q --short HEAD || true)
  reason=$(keep_reason "$lane")
  idle_days=$(((now - $(newest_mtime "$lane")) / 86400))
  if [[ -n $reason ]]; then
    echo "kept $name: $reason"; kept=$((kept + 1)); continue
  fi
  if [[ $dry == 1 ]]; then
    echo "would remove $name (branch ${branch:-none}, idle ${idle_days}d)"
    removed=$((removed + 1)); continue
  fi
  git -C "$main" worktree remove "$lane"
  note=""
  if [[ -n $branch ]]; then
    if ! git -C "$main" branch -d "$branch" >/dev/null 2>&1; then note="; branch $branch kept: git refuses a safe delete"; fi
  fi
  git -C "$main" worktree prune
  echo "removed $name (branch ${branch:-none}, idle ${idle_days}d)$note"
  removed=$((removed + 1))
done < <(printf '%s\n' "$listing" | awk -v d="$lanes_dir/" '/^worktree / { p = substr($0, 10); if (index(p, d) == 1) print p }')

for o in ${only[@]+"${only[@]}"}; do
  case $seen in *" $o "*) ;; *) echo "kept $o: no such lane"; kept=$((kept + 1)) ;; esac
done
if [[ $dry == 1 ]]; then verb="would remove"; else verb="removed"; fi
echo "$verb $removed, kept $kept"
