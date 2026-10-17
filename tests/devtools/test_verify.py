"""tools/devtools/verify.py: routing and the python-tests registry, without git or a subprocess."""

from __future__ import annotations

from pathlib import Path

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.devtools import verify  # noqa: E402


def test_rust_tests_command_has_no_package_flags() -> None:
    cmd = verify.rust_tests_command()
    assert "-p" not in cmd
    assert cmd[-3:] == ["-m", "tools.devtools.rust_ws", "test"]


def test_docs_select_no_steps() -> None:
    assert verify.steps_for(["docs/x.md"]) == ()
    assert verify.steps_for(["AGENTS.md"]) == ()


def test_changed_pk_test_runs_itself_and_the_gate() -> None:
    assert "python-tests" in verify.steps_for(["tests/patchkit/test_pk_cf.py"])
    tests = verify.python_tests(["tests/patchkit/test_pk_cf.py"], False)
    assert any(t in tests for t in ("tests/patchkit", "tests/patchkit/test_pk_cf.py"))
    assert set(verify.PY_GATE) <= set(tests)


def test_pk_harness_triggers_the_patchkit_suite() -> None:
    tests = verify.python_tests(["tools/patchkit/pk_harness.py"], False)
    assert "tests/patchkit" in tests


def test_digi_client_triggers_its_test() -> None:
    assert "tests/control" in verify.python_tests(
        ["tools/control/digi_client.py"], False
    )
    assert "python-tests" in verify.steps_for(["tools/control/digi_client.py"])
    assert "python-tests" in verify.steps_for(["packages/emulator/control/src/lib.rs"])


def test_moved_paths_route() -> None:
    assert "web" in verify.steps_for(["apps/web/src/x.ts"])
    assert "desktop" in verify.steps_for(["apps/desktop/src-tauri/src/main.rs"])
    assert "dt2-tone" in verify.steps_for(["packages/emulator/sharc/src/lib.rs"])
    assert "tier-quick" in verify.steps_for(
        ["packages/emulator/sharc-fast-cl/src/lib.rs"]
    )
    assert "web" in verify.steps_for(["packages/patchkit/src/lib.rs"])
    assert "card" in verify.steps_for(["packages/emulator/plusdrive/src/lib.rs"])
    assert "dt2-tone" in verify.steps_for(["packages/emulator/machine/src/lib.rs"])


def test_full_and_empty() -> None:
    assert verify.python_tests([], False) == list(verify.PY_GATE)
    full = verify.python_tests([], True)
    for _, _, paths in verify.PY_SUITES:
        assert set(verify.resolve_tests(paths)) <= set(full)
    assert len(full) == len(set(full))


def test_every_suite_entry_resolves() -> None:
    for name, _, paths in verify.PY_SUITES:
        for path in paths:
            assert verify.resolve_tests((path,)), (name, path)


def test_unmatched_entry_is_an_error() -> None:
    with pytest.raises(SystemExit):
        verify.resolve_tests(("tests/test_no_such_*.py",))
    with pytest.raises(SystemExit):
        verify.resolve_tests(("tests/test_no_such_file.py",))


def test_scenario_tests_are_not_added() -> None:
    changed = ["tests/scenarios/test_dt2_fx.py"]
    assert "tests/scenarios/test_dt2_fx.py" not in verify.python_tests(changed, False)


def test_changed_support_module_runs_its_users() -> None:
    tests = verify.python_tests(["tests/support/sharc_swi.py"], False)
    assert "tests/sharc/semantics/test_sharc_software_interrupts.py" in tests
    assert "tests/sharc/semantics/test_sharc_core_timer.py" in tests
    assert "tests/sharc/trace/test_sharc_trace_control.py" not in tests


def _group(
    label: str, stem: str, count: int, factor: float
) -> tuple[str, str, list[str], dict[str, float], float]:
    names = [f"{stem}-{i:03d}" for i in range(count)]
    seconds = {n: 4.0 + (i % 3) for i, n in enumerate(names)}
    return label, f"tests/scenarios/{stem}.json", names, seconds, factor


def test_plan_chunks() -> None:
    groups = [
        _group("default", "a", 47, 1.0),
        _group("default", "b", 87, 1.0),
        _group("no-aot", "a", 47, 2.0),
        _group("no-aot", "b", 87, 2.0),
    ]
    budget = 12
    chunks = verify.plan_chunks(groups, budget)
    for label, path, names, _, _ in groups:
        got = [n for c in chunks if (c.label, c.path) == (label, path) for n in c.names]
        assert sorted(got) == sorted(names)
        indexes = [c.index for c in chunks if (c.label, c.path) == (label, path)]
        assert sorted(indexes) == list(range(len(indexes)))
    costs = [c.cost for c in chunks]
    assert costs == sorted(costs, reverse=True)
    total = sum(costs)
    target = total / (budget * 4)
    biggest = max(f * s for _, _, _, sec, f in groups for s in sec.values())
    assert 24 <= len(chunks) <= 80
    assert max(costs) <= 2 * target + biggest


def test_plan_chunks_single_scenario() -> None:
    chunks = verify.plan_chunks([_group("default", "a", 1, 1.0)], 12)
    assert len(chunks) == 1
    assert chunks[0].names == ("a-000",)


def test_tier_jobs_budget_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("DIGI_TIER_JOBS", "7")
    assert verify.tier_jobs_budget() == 7
    monkeypatch.setenv("DIGI_TIER_JOBS", "0")
    with pytest.raises(SystemExit):
        verify.tier_jobs_budget()


def test_python_tests_passed(tmp_path: Path) -> None:
    head = "file\tstatus\trc\tseconds\tpassed\tfailed\tskipped\terrors\n"
    (tmp_path / "summary.tsv").write_text(head + "a.py\tOK\t0\t1.0\t0\t0\t3\t0\n")
    assert verify.python_tests_passed(tmp_path)
    (tmp_path / "summary.tsv").write_text(head + "a.py\tOK\t0\t1.0\t2\t0\t3\t0\n")
    assert verify.python_tests_passed(tmp_path) is None


def test_python_tests_command() -> None:
    cmd = verify.python_tests_command(Path("x"), ["tests/a.py"])
    assert "tools.devtools.test_outcomes" in cmd
    assert "run" in cmd and "--check" in cmd
    assert "--timeout" in cmd
    assert cmd[-1] == "tests/a.py"


def test_python_timeout_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("DIGI_PY_TIMEOUT", raising=False)
    assert verify.python_timeout() == 3600
    monkeypatch.setenv("DIGI_PY_TIMEOUT", "120")
    assert verify.python_timeout() == 120
    cmd = verify.python_tests_command(Path("x"), ["tests/a.py"])
    assert cmd[cmd.index("--timeout") + 1] == "120"
    monkeypatch.setenv("DIGI_PY_TIMEOUT", "0")
    with pytest.raises(SystemExit):
        verify.python_timeout()


def test_load_last_result_picks_newest(tmp_path: Path) -> None:
    assert verify.load_last_result(tmp_path / "none", None) is None
    for stamp in ("20260101-000000", "20260102-000000", "20260103-000000"):
        (tmp_path / stamp).mkdir()
    (tmp_path / "20260101-000000" / "result.json").write_text('{"n": 1}')
    (tmp_path / "20260102-000000" / "result.json").write_text('{"n": 2}')
    assert verify.load_last_result(tmp_path, None) == {"n": 2}
    assert verify.load_last_result(tmp_path, tmp_path / "20260102-000000") == {"n": 1}


def test_rerun_plan() -> None:
    result = {"failed": ["pins", "check"], "py_tests": ["tests/a.py"]}
    expected = tuple(s for s in verify.STEPS if s in {"pins", "check"})
    assert verify.rerun_plan(result) == (expected, ["tests/a.py"])
    with pytest.raises(SystemExit):
        verify.rerun_plan({"failed": ["bogus"], "py_tests": []})


def test_rerun_failed_excludes_only_and_full() -> None:
    with pytest.raises(SystemExit) as info:
        verify.main(["--rerun-failed", "--only", "pins"])
    assert info.value.code == 2
    with pytest.raises(SystemExit) as info:
        verify.main(["--rerun-failed", "--full"])
    assert info.value.code == 2


def test_pick_rechecks() -> None:
    names = [f"s{i}" for i in range(20)]
    rng = verify.coupled.random.Random(1)
    assert verify.coupled.pick_rechecks(names, 0.0, rng) == []
    assert verify.coupled.pick_rechecks(names, 1.0, rng) == names
    first = verify.coupled.pick_rechecks(names, 0.5, verify.coupled.random.Random(7))
    again = verify.coupled.pick_rechecks(names, 0.5, verify.coupled.random.Random(7))
    assert first == again
    assert 0 < len(first) < len(names)
    assert set(first) <= set(names)


def test_recheck_probability_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("DIGI_RENDER_CACHE_CHECK", raising=False)
    assert verify.coupled.recheck_probability() == 0.0
    monkeypatch.setenv("DIGI_RENDER_CACHE_CHECK", "0.25")
    assert verify.coupled.recheck_probability() == 0.25
    for bad in ("2", "-0.1", "x", "nan"):
        monkeypatch.setenv("DIGI_RENDER_CACHE_CHECK", bad)
        with pytest.raises(SystemExit):
            verify.coupled.recheck_probability()


def test_cache_env() -> None:
    env = verify.cache_env({"A": "1"})
    assert env == {"A": "1", "DIGI_RENDER_CACHE_CHECK": "0.03"}
    keep = verify.cache_env({"DIGI_RENDER_CACHE_CHECK": "0.5"})
    assert keep["DIGI_RENDER_CACHE_CHECK"] == "0.5"
    off = verify.cache_env({}, no_cache=True)
    assert off["DIGI_RENDER_CACHE"] == "0"
    assert "DIGI_RENDER_CACHE" not in env


def test_explain_runs_nothing(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def fail(*args: object, **kwargs: object) -> None:
        raise AssertionError("a run directory was created")

    monkeypatch.setattr(verify.digi_env, "run_dir", fail)
    assert verify.main(["--explain", "--only", "pins"]) == 0
    assert "pins" in capsys.readouterr().out


def test_plaits_suite_runs_the_oracle_tests() -> None:
    for changed in (
        "modules/machines/plaits/src/dsp/engines/va.c",
        "tests/plaits/oracle/va_oracle.cc",
        "tests/support/plaits_host.py",
    ):
        plan = dict(verify.python_test_plan([changed], False))
        assert plan.get("tests/plaits") == "plaits", changed
        assert "python-tests" in verify.steps_for([changed])


def test_python_tests_require_the_plaits_oracle_unless_told_otherwise() -> None:
    assert verify.python_tests_env({})["PLAITS_ORACLE_REQUIRED"] == "1"
    env = verify.python_tests_env({"PLAITS_ORACLE_REQUIRED": "0"})
    assert env["PLAITS_ORACLE_REQUIRED"] == "0"
