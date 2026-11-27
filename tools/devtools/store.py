"""Content-addressed store of build results, shared by every worktree of the clone.

    <git common dir>/<name>/<group>/<key>/        one entry: the artifact files and `key.txt`

Two users: the daemon store (`emud-cache`, group = device, tools/devtools/verify_build.py) and the core
library store (`sharc-core-cache`, group = image, tools/devtools/core_store.py). The rules are the same:

- An entry is built in `<group>/.tmp-<pid>` and inserted with one rename, so a reader sees a whole entry or none.
  When two processes insert the same key, one rename wins and the other uses the winner's entry.
- The use time of an entry is the mtime of its `key.txt`: set on insert, refreshed on every restore.
- Eviction keeps the KEEP most recently used keys of a group, the key just used, every entry used less than an hour ago,
  and every key that `dist/daemons.json` of any worktree of the clone points at (a value `<store>/<group>/<key>/<file>`).
  An entry is renamed to `.tmp-evict-*` before it is removed, so a reader that opened it keeps a whole entry or finds none.
  Stale `.tmp-*` directories (over an hour old) are removed too.
"""

from __future__ import annotations

import json
import os
import secrets
import shutil
import subprocess
import time
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

KEEP = 4  # most recently used keys kept per group


def store_root(name: str, override_env: str) -> Path:
    """`<git common dir>/NAME`, or the path in the environment variable OVERRIDE_ENV (tests only)."""
    override = os.environ.get(override_env)
    if override:
        return Path(override)
    done = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    )  # fmt: skip
    return Path(done.stdout.strip()) / name


def worktree_roots() -> list[Path]:
    """The root directory of every worktree of the clone (`git worktree list --porcelain`)."""
    done = subprocess.run(
        ["git", "worktree", "list", "--porcelain"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    )  # fmt: skip
    return [
        Path(line.removeprefix("worktree "))
        for line in done.stdout.splitlines()
        if line.startswith("worktree ")
    ]


def referenced_keys(group_dir: Path, worktrees: list[Path]) -> set[str]:
    """The keys of GROUP_DIR that `dist/daemons.json` of any of WORKTREES points at.

    A worktree without that file, or with one that cannot be read, references nothing.
    """
    keys: set[str] = set()
    for tree in worktrees:
        try:
            pinned = json.loads((tree / "dist" / "daemons.json").read_text())
        except (OSError, ValueError):
            continue
        if not isinstance(pinned, dict):
            continue
        for value in pinned.values():
            binary = Path(str(value))
            if binary.parent.parent == group_dir:
                keys.add(binary.parent.name)
    return keys


FRESH_SECONDS = 3600  # an entry used less than this long ago is never evicted
STALE_TEMP_SECONDS = (
    3600  # a `.tmp-*` directory older than this is a dead insert or eviction
)


def remove(entry: Path) -> None:
    """Delete ENTRY so that no reader sees half of it: rename it to `.tmp-evict-<pid>-<random>`, then remove the copy."""
    aside = entry.parent / f".tmp-evict-{os.getpid()}-{secrets.token_hex(4)}"
    try:
        os.rename(entry, aside)
    except OSError:
        return  # gone already, or another process removes it
    shutil.rmtree(aside, ignore_errors=True)


def mark_used(entry: Path) -> bool:
    """Refresh the use time of ENTRY (mtime of its existing `key.txt`). False when there is no `key.txt`: the entry is not whole.

    Never creates the file."""
    try:
        os.utime(entry / "key.txt")
    except OSError:
        return False
    return True


def evict(
    group_dir: Path,
    keep_key: str,
    referenced: set[str] | frozenset[str] = frozenset(),
    now: float | None = None,
) -> list[str]:
    """Keep the KEEP most recently used keys of a group, KEEP_KEY, every key in REFERENCED and every entry used less than
    FRESH_SECONDS ago; remove the others (see `remove`) and return their names.

    The use time is the mtime of key.txt: it is set on insert and refreshed on every restore.
    Also removes `.tmp-*` and `.tmp-evict-*` directories older than STALE_TEMP_SECONDS.
    """
    now = time.time() if now is None else now
    entries = []
    for entry in group_dir.iterdir():
        if not entry.is_dir():
            continue
        if entry.name.startswith("."):
            try:
                if entry.name.startswith(".tmp-") and (
                    now - entry.stat().st_mtime > STALE_TEMP_SECONDS
                ):
                    shutil.rmtree(entry, ignore_errors=True)
            except OSError:
                pass
            continue
        try:
            entries.append((entry.joinpath("key.txt").stat().st_mtime, entry))
        except OSError:
            entries.append((0.0, entry))
    entries.sort(key=lambda item: item[0], reverse=True)
    removed = []
    for used, entry in entries[KEEP:]:
        if (
            entry.name != keep_key
            and entry.name not in referenced
            and now - used >= FRESH_SECONDS
        ):
            remove(entry)
            removed.append(entry.name)
    return removed


def insert(
    group_dir: Path, key: str, marker: str, fill: Callable[[Path], None]
) -> bool:
    """Insert the entry KEY of GROUP_DIR: FILL(tmp) writes the files, `key.txt` is added, one rename publishes it.

    True when this call inserted the entry; False when another process inserted the same key first
    (MARKER is the file whose presence says the winner's entry is complete; this call's files are removed).
    """
    final = group_dir / key
    group_dir.mkdir(parents=True, exist_ok=True)
    tmp = group_dir / f".tmp-{os.getpid()}"
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir()
    fill(tmp)
    (tmp / "key.txt").write_text(key + "\n")
    try:
        os.rename(tmp, final)
        return True
    except OSError:
        if not (final / marker).is_file():
            raise
        shutil.rmtree(tmp, ignore_errors=True)
        return False
