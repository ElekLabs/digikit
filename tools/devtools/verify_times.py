"""How long each python-tests file took in earlier verify runs (the order of its jobs: longest first).

The record is the one verify already writes: out/runs/verify/<run>/python-tests/summary.tsv. Nothing else is kept.
"""

from __future__ import annotations

import contextlib
from collections.abc import Iterable
from pathlib import Path

DEFAULT_SECONDS = 60.0  # a file no earlier run has timed
RUNS_SCANNED = 12  # the newest runs read


def run_seconds(run: Path, files: Iterable[str]) -> dict[str, float]:
    """Seconds per file of one verify run directory (only the files in FILES)."""
    out = run / "python-tests"
    found: dict[str, float] = {}
    summary = out / "summary.tsv"
    wanted = set(files)
    if summary.is_file():
        for row in summary.read_text().splitlines()[1:]:
            cols = row.split("\t")
            if len(cols) > 3 and cols[0] in wanted and cols[1] == "OK":
                with contextlib.suppress(ValueError):
                    found[cols[0]] = float(cols[3])
        return found
    return found


def failed_files(run: Path) -> list[str]:
    """The python-tests files of one verify run directory whose status is not OK."""
    summary = run / "python-tests" / "summary.tsv"
    if not summary.is_file():
        return []
    rows = (row.split("\t") for row in summary.read_text().splitlines()[1:])
    return [cols[0] for cols in rows if len(cols) > 1 and cols[1] != "OK"]


def file_seconds(
    verify_dir: Path, files: Iterable[str], exclude: Path | None = None
) -> dict[str, float]:
    """The newest known seconds of each of FILES over the newest runs under VERIFY_DIR, EXCLUDE (the
    run being made) skipped."""
    files = list(files)
    known: dict[str, float] = {}
    if not verify_dir.is_dir():
        return known
    runs = [
        r
        for r in sorted(verify_dir.iterdir(), key=lambda p: p.name, reverse=True)
        if r.is_dir()
        and not r.is_symlink()
        and (exclude is None or r.resolve() != exclude.resolve())
    ]
    for run in runs[:RUNS_SCANNED]:
        for rel, seconds in run_seconds(run, files).items():
            known.setdefault(rel, seconds)
        if len(known) == len(files):
            break
    return known


def step_seconds(
    runs_root: Path, steps: list[str], current: Path | None = None
) -> dict[str, float]:
    """The newest known seconds of each of STEPS (a node of jobs.tsv: end_s - start_s) over the newest
    runs under RUNS_ROOT, CURRENT (the run being made) skipped. A step no run timed is left out."""
    known: dict[str, float] = {}
    if not runs_root.is_dir():
        return known
    runs = [
        r
        for r in sorted(runs_root.iterdir(), key=lambda p: p.name, reverse=True)
        if r.is_dir()
        and not r.is_symlink()
        and (current is None or r.resolve() != current.resolve())
    ]
    wanted = set(steps)
    for run in runs[:RUNS_SCANNED]:
        try:
            rows = (run / "jobs.tsv").read_text().splitlines()[1:]
        except (OSError, UnicodeDecodeError):
            continue
        for row in rows:
            cols = row.split("\t")
            if len(cols) > 2 and cols[0] in wanted and cols[0] not in known:
                with contextlib.suppress(ValueError):
                    known[cols[0]] = float(cols[2]) - float(cols[1])
        if len(known) == len(wanted):
            break
    return known


def longest_first(
    files: list[str], seconds: dict[str, float]
) -> list[tuple[str, float, bool]]:
    """(file, estimate, known) longest first; a file without a time counts DEFAULT_SECONDS, and equal
    estimates keep the plan order."""
    rows = [(f, seconds.get(f, DEFAULT_SECONDS), f in seconds) for f in files]
    return sorted(rows, key=lambda r: -r[1])  # sorted is stable
