"""Report or retire old per-worktree Cargo scratch, without removing worktrees."""

from __future__ import annotations

import argparse
import getpass
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = Path("dist/native-target")
GIB = 1024**3
DAY = 24 * 3600
SHELLS = {"bash", "dash", "fish", "ksh", "sh", "tcsh", "zsh"}


@dataclass(frozen=True)
class Lane:
    root: Path
    locked: bool = False

    @property
    def target(self) -> Path:
        return self.root / TARGET


@dataclass(frozen=True)
class Scratch:
    lane: Lane
    size: int = 0
    latest: float = 0
    obstacle: str = ""
    identity: tuple[int, int] | None = None


def git(root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "--no-optional-locks", "-C", str(root), *args],
        capture_output=True,
        check=check,
        text=True,
    )


def worktrees(root: Path) -> list[Lane]:
    result = []
    for entry in git(root, "worktree", "list", "--porcelain", "-z").stdout.split(
        "\0\0"
    ):
        first, *fields = entry.split("\0")
        if first.startswith("worktree ") and not any(
            field.startswith("prunable") for field in fields
        ):
            result.append(
                Lane(
                    Path(first[9:]), any(field.startswith("locked") for field in fields)
                )
            )
    return result


def open_paths(output: str) -> set[Path]:
    """A shell's current directory alone does not pin a warm lane."""
    paths = set()
    command = fd = ""
    for field in output.split("\0"):
        field = field.lstrip("\n")
        if field.startswith("p"):
            command = fd = ""
        elif field.startswith("c"):
            command = field[1:]
        elif field.startswith("f"):
            fd = field[1:]
        elif field.startswith("n/") and not (fd == "cwd" and command in SHELLS):
            paths.add(Path(field[1:].removesuffix(" (deleted)")))
    return paths


def paths_in_use() -> set[Path]:
    """Absolute open paths and cwd of the current user's processes, or fail closed."""
    scan = subprocess.run(
        ["lsof", "-w", "-nP", "-a", "-u", getpass.getuser(), "-F0pcfn"],
        capture_output=True,
        text=True,
        errors="surrogateescape",
        timeout=30,
        check=True,
    )
    if "\nfcwd\0n/" not in scan.stdout:
        raise ValueError("no process working directories visible in lsof")
    return open_paths(scan.stdout)


def contains(path: Path, paths: Iterable[Path]) -> bool:
    return any(other.is_relative_to(path) for other in paths)


def measure(lane: Lane, roots: Iterable[Path]) -> Scratch | None:
    """Walk the actual target directory, without following aliases or mounts."""
    target = lane.target
    if not os.path.lexists(target):
        return None
    if target.resolve() != target or not target.is_dir():
        return Scratch(lane, obstacle="not a real lane target directory")
    if contains(target, roots):
        return Scratch(lane, obstacle="contains a registered worktree")
    top = target.stat()
    pending = [target]
    size = top.st_blocks * 512
    latest = max(top.st_mtime, top.st_ctime)
    hardlinks = set()
    while pending:
        with os.scandir(pending.pop()) as entries:
            for entry in entries:
                data = entry.stat(follow_symlinks=False)
                if data.st_dev != top.st_dev or entry.name == ".git":
                    return Scratch(lane, obstacle="contains a mount or Git checkout")
                if stat.S_ISDIR(data.st_mode):
                    pending.append(Path(entry.path))
                elif data.st_nlink > 1:
                    if data.st_ino in hardlinks:
                        continue
                    hardlinks.add(data.st_ino)
                size += data.st_blocks * 512
                latest = max(latest, data.st_mtime, data.st_ctime)
    return Scratch(lane, size, latest, identity=(top.st_dev, top.st_ino))


def protection(
    scratch: Scratch, active: set[Path], keep: set[str], cutoff: float
) -> str:
    lane = scratch.lane
    if scratch.obstacle:
        return scratch.obstacle
    if lane.locked:
        return "Git worktree is locked"
    if {lane.root.name, str(lane.root)} & keep or Path.cwd().is_relative_to(lane.root):
        return "kept worktree"
    if contains(lane.root, active):
        return "open files or an active process"
    if scratch.latest >= cutoff:
        return "recent build writes"
    if git(lane.root, "ls-files", "--", str(TARGET)).stdout:
        return "target contains tracked files"
    if git(lane.root, "check-ignore", "--", str(TARGET), check=False).returncode:
        return "not a Git-ignored target directory"
    if git(lane.root, "status", "--porcelain").stdout:
        return "worktree has uncommitted changes"
    return ""


def select(
    scratches: list[Scratch], reasons: dict[Lane, str], budget: int | None
) -> list[Scratch]:
    """Oldest eligible targets first. A budget never overrides a protection."""
    remaining = sum(s.size for s in scratches)
    remove = []
    for scratch in sorted(scratches, key=lambda s: (s.latest, s.lane.root)):
        if budget is not None and remaining <= budget:
            break
        if not reasons[scratch.lane]:
            remove.append(scratch)
            remaining -= scratch.size
    return remove


def retire(scratch: Scratch) -> None:
    """Detach the old tree first so cleanup cannot follow a freshly rebuilt target."""
    target = scratch.lane.target
    data = target.lstat()
    if target.resolve() != target or (data.st_dev, data.st_ino) != scratch.identity:
        raise ValueError(f"target changed during cleanup: {target}")
    aside = Path(tempfile.mkdtemp(prefix=".native-target-clean-", dir=target.parent))
    try:
        target.rename(aside / "target")
    except OSError:
        aside.rmdir()
        raise
    shutil.rmtree(aside)


def valid_days(raw: str) -> float:
    import math

    value = float(raw)
    if not math.isfinite(value) or value < 0:
        raise argparse.ArgumentTypeError("expected a nonnegative finite number")
    return value


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--idle-days", type=valid_days, default=7)
    parser.add_argument(
        "--max-gib", type=valid_days, help="stop when the selected lanes fit the budget"
    )
    parser.add_argument("--keep", action="append", default=[], metavar="LANE")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument(
        "lanes", nargs="*", help="optional lane names or absolute paths"
    )
    args = parser.parse_args(argv)
    cutoff = time.time() - args.idle_days * DAY
    budget = None if args.max_gib is None else int(args.max_gib * GIB)
    keep = set(args.keep)
    try:
        main_lane, *lanes = worktrees(ROOT)
        roots = [lane.root for lane in [main_lane, *lanes]]
        selected = set(args.lanes)
        if unknown := selected - {
            n for lane in lanes for n in (lane.root.name, str(lane.root))
        }:
            parser.error(f"unknown linked worktree(s): {', '.join(sorted(unknown))}")
        for name in selected:
            if sum(name in (lane.root.name, str(lane.root)) for lane in lanes) > 1:
                parser.error(f"ambiguous worktree name {name!r}; use its absolute path")
        active = paths_in_use()
        scratches = []
        reasons = {}
        for lane in lanes:
            if selected and not ({lane.root.name, str(lane.root)} & selected):
                continue
            if (scratch := measure(lane, roots)) is not None:
                scratches.append(scratch)
                reasons[lane] = protection(scratch, active, keep, cutoff)
        removal = select(scratches, reasons, budget)
        planned = {s.lane for s in removal}
        for scratch in sorted(scratches, key=lambda s: (-s.size, s.lane.root)):
            why = reasons[scratch.lane] or "within budget"
            label = "eligible" if scratch.lane in planned else f"keep ({why})"
            print(f"{label:35} {scratch.size / GIB:7.1f} GiB  {scratch.lane.target}")
        total = sum(s.size for s in scratches)
        freed = sum(s.size for s in removal)
        print(f"{total / GIB:.1f} GiB inventoried, {freed / GIB:.1f} GiB eligible")
        if budget is not None and total - freed > budget:
            print("Budget cannot be reached without touching protected targets.")
        if not args.apply:
            print("Dry run. Add --apply to retire the eligible Cargo targets.")
            return 0
        removed = 0
        for scratch in removal:
            current = worktrees(ROOT)
            lane = next((w for w in current[1:] if w.root == scratch.lane.root), None)
            if lane is None:
                print(f"skip {scratch.lane.target}: lane is no longer registered")
                continue
            checked = measure(lane, [w.root for w in current])
            reason = "target changed during cleanup"
            if checked == scratch and checked is not None:
                reason = protection(checked, paths_in_use(), keep, cutoff)
                if not reason:
                    retire(checked)
                    removed += checked.size
                    print(f"removed {lane.target}", flush=True)
                    continue
            print(f"skip {lane.target}: {reason}")
        print(f"Retired {removed / GIB:.1f} GiB of measured build output.")
        return 0
    except (OSError, subprocess.SubprocessError, ValueError) as exc:
        print(f"lane cleanup stopped: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
