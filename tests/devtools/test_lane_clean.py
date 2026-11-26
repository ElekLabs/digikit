from __future__ import annotations

import time
from pathlib import Path

import pytest

from tools.devtools import lane_clean


def test_idle_shell_cwd_does_not_pin_but_open_files_do() -> None:
    output = (
        "p1\0czsh\0\nfcwd\0n/lane-a\0\nf8\0n/lane-b/build.log\0\n"
        "p2\0ccargo\0\nfcwd\0n/lane-c\0\n"
    )
    assert lane_clean.open_paths(output) == {
        Path("/lane-b/build.log"),
        Path("/lane-c"),
    }


def test_budget_evicts_oldest_without_using_protected_targets(tmp_path: Path) -> None:
    a, b, c = (lane_clean.Lane(tmp_path / n) for n in ("a", "b", "c"))
    blocked = lane_clean.Scratch(a, 100, 1)
    oldest = lane_clean.Scratch(b, 20, 2)
    newer = lane_clean.Scratch(c, 20, 3)
    sizes = [newer, oldest, blocked]
    reasons = {a: "active", b: "", c: ""}
    assert lane_clean.select(sizes, reasons, 120) == [oldest]
    assert lane_clean.select(sizes, reasons, 0) == [oldest, newer]


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    root = (tmp_path / "main").resolve()
    root.mkdir()
    lane_clean.git(root, "init", "-q")
    (root / ".gitignore").write_text("/dist/\n")
    lane_clean.git(root, "add", ".gitignore")
    lane_clean.git(
        root,
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@example.invalid",
        "commit",
        "-qm",
        "fixture",
    )
    monkeypatch.setattr(lane_clean, "ROOT", root)
    future = time.time() + 30 * lane_clean.DAY
    monkeypatch.setattr(lane_clean.time, "time", lambda: future)
    return root


def lane(repo: Path, name: str) -> lane_clean.Lane:
    path = repo.parent / name
    lane_clean.git(repo, "worktree", "add", "-q", "--detach", str(path))
    wt = lane_clean.Lane(path)
    wt.target.mkdir(parents=True)
    (wt.target / "lib.rlib").write_bytes(b"x")
    return wt


def test_dry_run_then_apply_preserves_worktrees_and_protected_targets(
    repo: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    idle = lane(repo, "idle")
    busy = lane(repo, "busy")
    pinned = lane(repo, "pinned")
    locked = lane(repo, "locked")
    dirty = lane(repo, "dirty")
    alias = lane(repo, "alias")
    wanted = repo / "dist/native-target"
    wanted.mkdir(parents=True)
    (alias.target / "lib.rlib").unlink()
    alias.target.rmdir()
    alias.target.symlink_to(wanted, target_is_directory=True)
    lane_clean.git(repo, "worktree", "lock", str(locked.root))
    (dirty.root / "notes.txt").write_text("work in progress")
    monkeypatch.setattr(lane_clean, "paths_in_use", lambda: {busy.root / "build.log"})

    assert lane_clean.main(["--keep", "pinned"]) == 0
    assert idle.target.is_dir()
    assert "Dry run" in capsys.readouterr().out
    assert lane_clean.main(["--apply", "--keep", "pinned"]) == 0
    assert not idle.target.exists()
    assert (idle.root / ".git").is_file()
    assert all(w.target.is_dir() for w in (busy, pinned, locked, dirty, alias))
    assert wanted.is_dir()


def test_apply_rechecks_a_lane_that_became_busy(
    repo: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    idle = lane(repo, "idle")
    scans = iter([set(), {idle.target / "lib.rlib"}])
    monkeypatch.setattr(lane_clean, "paths_in_use", lambda: next(scans))

    assert lane_clean.main(["--apply"]) == 0
    assert idle.target.is_dir()
    assert "active process" in capsys.readouterr().out


def test_apply_fails_closed_without_process_visibility(
    repo: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    idle = lane(repo, "idle")

    def unavailable() -> set[Path]:
        raise OSError("lsof unavailable")

    monkeypatch.setattr(lane_clean, "paths_in_use", unavailable)
    assert lane_clean.main(["--apply"]) == 1
    assert idle.target.is_dir()
