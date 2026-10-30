"""Tests for digi_env.run_dir pruning (firmware-free)."""

from __future__ import annotations

from tools.emulator import digi_env


def test_run_dir_prunes_old_runs(tmp_path, monkeypatch):
    monkeypatch.setattr(digi_env, "RUNS_ROOT", tmp_path)
    base = tmp_path / "kind"
    base.mkdir()
    for i in range(digi_env.KEEP_RUNS + 5):
        (base / f"20200101-{i:06d}").mkdir()
    (base / "other").mkdir()
    new = digi_env.run_dir("kind", stamp="20300101-000000")
    names = sorted(p.name for p in base.iterdir())
    stamped = [n for n in names if n[0].isdigit()]
    assert len(stamped) == digi_env.KEEP_RUNS
    assert new.name in stamped
    assert "20200101-000000" not in stamped
    assert f"20200101-{digi_env.KEEP_RUNS + 4:06d}" in stamped
    assert "other" in names
    assert (base / "latest").is_symlink()
    assert (base / "latest").resolve() == new.resolve()


def test_run_dir_gives_each_call_with_the_same_stamp_its_own_directory(
    tmp_path, monkeypatch
):
    monkeypatch.setattr(digi_env, "RUNS_ROOT", tmp_path)
    (tmp_path / "kind" / "20300101-000000").mkdir(parents=True)
    a = digi_env.run_dir("kind", stamp="20300101-000000")
    b = digi_env.run_dir("kind", stamp="20300101-000000")
    assert a.name == "20300101-000000-2"
    assert b.name == "20300101-000000-3"
    assert a.is_dir() and b.is_dir()
