#!/usr/bin/env bash
# A persistent lane worktree for parallel agent work. Lanes are reused so their
# Rust build directories stay warm (a fresh worktree pays about 25 minutes of
# cold builds).
#
#   tools/devtools/lane.sh NAME [BASE]     # prints the lane's path
#   tools/devtools/lane.sh --prune [--days N] [--dry-run] [--only NAME]...
#                                          # removes finished lanes (lane-prune.sh)
#
# The lane lives at ../lanes/NAME next to the main tree, on branch lane/NAME.
# On reuse it is reset to BASE (default: the main tree's current branch) when it
# has no uncommitted changes and no commits that BASE lacks.
set -euo pipefail

if [[ ${1:-} == --prune ]]; then
  shift
  exec "$(dirname "${BASH_SOURCE[0]}")/lane-prune.sh" "$@"
fi

name=${1:?usage: tools/devtools/lane.sh NAME [BASE]}
# Read the whole listing (see tools/devtools/worktree-setup.sh about SIGPIPE).
main=$(git worktree list --porcelain | awk '/^worktree / && !n {print substr($0, 10); n=1}')
base=${2:-$(git -C "$main" rev-parse --abbrev-ref HEAD)}
lane="$(dirname "$main")/lanes/$name"

if [[ ! -d $lane ]]; then
  mkdir -p "$(dirname "$lane")"
  git -C "$main" worktree add -q -B "lane/$name" "$lane" "$base"
else
  if [[ -n $(git -C "$lane" status --porcelain --untracked-files=no) ]]; then
    echo "error: lane $name has uncommitted changes" >&2
    exit 2
  fi
  if ! git -C "$main" merge-base --is-ancestor "lane/$name" "$base"; then
    echo "error: lane/$name has commits that $base lacks; merge them first" >&2
    exit 2
  fi
  git -C "$lane" checkout -q -B "lane/$name" "$base"
fi
(cd "$lane" && "$lane/tools/devtools/worktree-setup.sh" "$base" >&2)
mkdir -p "$main/out/runs/lanes"
echo "$lane"
