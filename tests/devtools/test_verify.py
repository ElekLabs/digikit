"""tools/devtools/verify.py: routing and the python-tests registry, without git or a subprocess."""

from __future__ import annotations

import json
import os
import re
import subprocess
from contextlib import nullcontext
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.devtools import changed_tests, verify, verify_route  # noqa: E402
from tools.patchkit import modroots  # noqa: E402


def test_docs_select_no_steps() -> None:
    assert verify.steps_for(["docs/x.md"]) == ()
    assert verify.steps_for(["AGENTS.md"]) == ()


UI_FILES = [
    "apps/desktop/ui/src/Console.tsx",
    "apps/desktop/ui/src/console.css",
    "apps/desktop/ui/preview/sim.ts",
    "apps/web/src/components/PatchGallery.tsx",
    "apps/web/src/styles/site.css",
    "apps/web/tests/gallery.test.mjs",
    "docs/TESTING.md",
]


def test_ui_only_change_skips_smoke() -> None:
    for path in UI_FILES:
        steps = verify.steps_for([path])
        assert "smoke" not in steps, path
    steps = verify.steps_for(UI_FILES)
    assert "smoke" not in steps
    assert "web" in steps  # the UI steps themselves still run


SMOKE_EXEMPT_FILES = [
    "tools/devtools/agent_guard.py",
    "tools/devtools/pathcheck.py",
    "tools/devtools/layercheck.py",
    ".claude/settings.json",
]

HOST_TEST_FILES = [
    "packages/control-client/tests/client.test.ts",
    "packages/test-support/tests/support.test.ts",
    "packages/dk-session/tests/remote.test.ts",
    "tests/regression/host/runtime.test.ts",
    "tests/regression/host/remote-helpers.test.ts",
    "apps/desktop/ui/tests/card.test.ts",
]


@pytest.mark.parametrize(
    "files, wanted, smoke, reason",
    [
        (UI_FILES, {"web"}, False, "UI-only change"),
        (SMOKE_EXEMPT_FILES, {"python-tests"}, False, "smoke-exempt maintenance"),
        (HOST_TEST_FILES, {"host"}, False, "host-only test"),
        (["packages/dk-session/tests/new.test.ts"], {"host"}, False, "host-only test"),
        (["tests/regression/host/new.test.ts"], {"host"}, False, "host-only test"),
        (["packages/dk-session/tests/fake.ts"], {"host"}, True, None),
        (["tests/regression/runtime/x.test.ts"], {"runtime-fixture"}, True, None),
        (["vitest.host.config.ts"], {"host"}, True, None),
        (
            [*HOST_TEST_FILES, "docs/x.md", "tools/devtools/pathcheck.py"],
            {"host", "python-tests"},
            False,
            "smoke-exempt maintenance change (host-only test",
        ),
        (
            [*HOST_TEST_FILES, "apps/web/src/components/PatchGallery.tsx"],
            {"host", "web"},
            False,
            "UI-only and host-only test",
        ),
        ([*HOST_TEST_FILES, "unknown/file.txt"], {"host"}, True, None),
        (
            [*HOST_TEST_FILES, "modules/machines/bd/src/module.c"],
            {"host"},
            True,
            None,
        ),
        ([*UI_FILES, "tools/device/dkscreen.py"], {"web"}, True, None),
        *[
            ([*HOST_TEST_FILES, path], {"host"}, True, None)
            for path in (
                "packages/control-client/src/client.ts",
                "packages/test-support/src/session.ts",
                "tests/regression/support.ts",
                "packages/control-client/package.json",
                "packages/test-support/tsconfig.json",
                "modules/core/runtime/src/rs/src/upload.rs",
            )
        ],
        (["tools/devtools/pathchecks.py"], set(), True, None),
        (
            ["tools/devtools/pathcheck.py", "apps/web/src/components/PatchGallery.tsx"],
            {"python-tests", "web"},
            False,
            "UI-only and smoke-exempt maintenance change (path checker)",
        ),
        ([*SMOKE_EXEMPT_FILES, "unknown/file.txt"], {"python-tests"}, True, None),
        (
            ["tools/devtools/pathcheck.py", "modules/machines/bd/src/module.c"],
            {"python-tests"},
            True,
            None,
        ),
    ],
)
def test_smoke_skip_table(
    files: list[str], wanted: set[str], smoke: bool, reason: str | None
) -> None:
    steps = verify.steps_for(files)
    assert wanted <= set(steps)
    assert ("smoke" in steps) == smoke
    if reason is None:
        assert verify.smoke_reason(files) is None
    else:
        assert reason in (verify.smoke_reason(files) or "")


def test_explain_states_why_host_tests_skip_smoke(
    capsys: pytest.CaptureFixture[str],
) -> None:
    files = ["packages/dk-session/tests/new.test.ts"]
    verify.explain(verify.steps_for(files), files, [], per_file=True)
    assert "smoke: skipped, host-only test change" in capsys.readouterr().out


@pytest.mark.parametrize("name", ["remote-page", "new"])
def test_desktop_host_tests_select_only_host(name: str) -> None:
    assert verify.steps_for([f"apps/desktop/ui/tests/{name}.test.ts"]) == ("host",)


@pytest.mark.parametrize(
    "path",
    [
        "apps/desktop/ui/src/remote-core.ts",
        "apps/desktop/package.json",
        "apps/desktop/ui/tsconfig.json",
        "apps/desktop/src-tauri/src/main.rs",
    ],
)
def test_desktop_host_sources_keep_app_checks(path: str) -> None:
    steps = set(verify.steps_for([path]))
    assert {"desktop", "host"} <= steps
    assert "web" not in steps


@pytest.mark.parametrize("name", ["session-core", "wait-core"])
def test_shared_transfer_sources_keep_desktop_and_web_checks(name: str) -> None:
    steps = verify.steps_for([f"apps/web/src/transfer/{name}.ts"])
    assert steps == ("desktop", "host", "web")


@pytest.mark.parametrize("name", ["card-core", "samples-core"])
def test_desktop_sources_used_by_web_samples_keep_web_checks(name: str) -> None:
    assert verify.steps_for([f"apps/desktop/ui/src/{name}.ts"]) == (
        "desktop",
        "host",
        "web",
    )


def test_exact_maintenance_paths_skip_smoke_but_keep_python_coverage() -> None:
    for path in SMOKE_EXEMPT_FILES:
        assert verify.steps_for([path]) == ("python-tests",)
        assert set(verify.PY_GATE) <= set(verify.python_tests([path], False))
    assert verify.steps_for(SMOKE_EXEMPT_FILES) == ("python-tests",)
    assert "tests/devtools/test_agent_guard.py" in verify.python_tests(
        ["tools/devtools/agent_guard.py"], False
    )
    assert "tests/devtools/test_pathcheck.py" in verify.python_tests(
        ["tools/devtools/pathcheck.py"], False
    )


@pytest.mark.parametrize("mid", ["digikit/freqshift", "digikit/remote"])
def test_audited_upload_without_matrix_consumers_skips_smoke(mid: str) -> None:
    files = [f"{mpath(mid)}/module.toml", *UI_FILES, "tools/devtools/pathcheck.py"]
    assert verify.smoke_cells(files) == []
    assert "smoke" not in verify.steps_for(files)
    assert "module-load" in verify.steps_for(files)
    assert "no dependent cells" in verify.smoke_reason(files)


def test_a_production_leaf_with_no_cell_skips_the_smoke_matrix() -> None:
    # the matrix cells hold the patched-in fixture, which has its own DSP copy: a change of pluck reaches no cell
    files = [f"{mpath('digikit/pluck')}/module.toml", "docs/TESTING.md"]
    assert verify.smoke_cells(files) == []
    assert "no dependent cells" in verify.smoke_reason(files)


def test_a_production_leaf_selects_existing_dependent_cells_on_both_devices(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setitem(verify_route.RUNTIME_UPLOADS, "test/patched-pluck", "x.test.ts")
    files = [f"{mpath('test/patched-pluck')}/module.toml", "docs/TESTING.md"]
    cells = verify.smoke_cells(files)
    assert cells is not None
    assert {cell.partition(":")[0] for cell in cells} == {"dt2-1.17", "dn2-1.12"}
    assert all("test/patched-pluck" in cell for cell in cells)
    assert "smoke" in verify.steps_for(files)
    # An unaudited/shared or unknown dependency keeps the entire matrix.
    for other in (
        "modules/core/runtime/module.toml",
        "tests/patchkit/smoke/matrix.toml",
        "unknown.file",
    ):
        assert verify.smoke_cells([*files, other]) is None
        assert "smoke" in verify.steps_for([*files, other])


@pytest.mark.parametrize(
    "selection", [None, ["dt2-1.17:core/dsp-voice,test/patched-pluck"]]
)
def test_smoke_forwards_native_cell_filters(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, selection: list[str] | None
) -> None:
    gate = verify.Gate(tmp_path, budget=2, smoke_selection=selection)
    calls = []
    monkeypatch.setattr(gate, "run", lambda name, cmd, **kw: calls.append(cmd))
    verify.run_step(gate, "smoke")
    expected = [*verify.PY, "-m", "tools.patchkit.pk_harness", "run", "--jobs", "1"]
    for cell in selection or []:
        expected += ["--cell", cell]
    assert calls == [expected]


def test_smoke_explain_names_affected_cells_and_empty_selection(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setitem(verify_route.RUNTIME_UPLOADS, "test/patched-pluck", "x.test.ts")
    files = [f"{mpath('test/patched-pluck')}/module.toml"]
    verify.explain(verify.steps_for(files), files, [], per_file=True)
    out = capsys.readouterr().out
    assert "selected dependent matrix cells" in out
    assert "test/patched-pluck" in out
    files = [f"{mpath('digikit/remote')}/module.toml"]
    verify.explain(verify.steps_for(files), files, [], per_file=True)
    assert (
        "smoke: skipped, audited production leaves have no dependent cells"
        in capsys.readouterr().out
    )


@pytest.mark.parametrize(
    "path",
    [
        "apps/desktop/src-tauri/src/main.rs",
        "apps/web/src/pages/patch.astro",
        "apps/web/src/modules/dktransfer.ts",
        "apps/web/src/patch/patchkit-core.ts",
        "modules/core/runtime/module.toml",
        "tests/patchkit/smoke/matrix.toml",
    ],
)
def test_non_ui_paths_keep_smoke(path: str) -> None:
    assert "smoke" in verify.steps_for([*UI_FILES, path])
    assert not verify.ui_only([path])


def test_docs_only_stays_empty() -> None:
    assert verify.steps_for(["docs/x.md", "README.md"]) == ()
    assert not verify.ui_only([])


def test_representative_module_runtime_and_core_routes_stay_conservative() -> None:
    assert verify.steps_for(["modules/machines/bd/src/module.c"]) == (
        "python-tests",
        "rust-tests",
        "smoke",
    )
    assert verify.steps_for(["modules/core/runtime/src/rs/src/upload.rs"]) == (
        "runtime-fixture",
        "module-load",
        "python-tests",
        "rust-tests",
        "smoke",
    )
    assert (
        verify.steps_for(["tools/sharc_dev/sharc_core/state.py"]) == verify.EVERYTHING
    )


def test_explain_says_why_smoke_is_skipped(capsys: pytest.CaptureFixture[str]) -> None:
    def run(files: list[str]) -> str:
        steps = verify.steps_for(files)
        verify.explain(steps, files, [], per_file=True)
        return capsys.readouterr().out

    assert "smoke: skipped, UI-only change" in run(UI_FILES)
    assert (
        "smoke: skipped, UI-only and smoke-exempt maintenance change (path checker)"
        in run(
            ["apps/web/src/components/PatchGallery.tsx", "tools/devtools/pathcheck.py"]
        )
    )
    assert "smoke: skipped" not in run([*UI_FILES, "tools/device/dkscreen.py"])
    assert "smoke: selected, conservative path: unknown/file.txt" in run(
        ["tools/devtools/pathcheck.py", "unknown/file.txt"]
    )
    assert "smoke: skipped" not in run(["docs/x.md"])
    assert "smoke: skipped, host-only test change" in run(
        ["packages/control-client/tests/client.test.ts"]
    )


def test_changed_pk_test_runs_itself_and_the_gate() -> None:
    assert "python-tests" in verify.steps_for(["tests/patchkit/test_pk_cf.py"])
    tests = verify.python_tests(["tests/patchkit/test_pk_cf.py"], False)
    assert any(t in tests for t in ("tests/patchkit", "tests/patchkit/test_pk_cf.py"))
    assert set(verify.PY_GATE) <= set(tests)


def test_claude_settings_run_the_agent_guard_test() -> None:
    assert verify.steps_for([".claude/settings.json"]) == ("python-tests",)
    plan = [p for p, _ in verify.python_test_plan([".claude/settings.json"], False)]
    assert "tests/devtools/test_agent_guard.py" in plan


def test_pathcheck_keeps_its_unit_test() -> None:
    plan = [
        p for p, _ in verify.python_test_plan(["tools/devtools/pathcheck.py"], False)
    ]
    assert "tests/devtools/test_pathcheck.py" in plan


def test_a_python_module_selects_its_importers_not_the_directory() -> None:
    tests = verify.python_tests(["tools/patchkit/pk_harness.py"], False)
    assert "tests/patchkit/smoke/test_pk_harness_units.py" in tests
    assert "tests/patchkit" not in tests


def test_a_rust_change_keeps_the_directory_suite() -> None:
    assert "tests/patchkit" in verify.python_tests(
        ["packages/patchkit/src/lib.rs"], False
    )


def test_digi_client_triggers_its_test() -> None:
    assert "tests/control/test_digi_client.py" in verify.python_tests(
        ["tools/control/digi_client.py"], False
    )
    assert "python-tests" in verify.steps_for(["tools/control/digi_client.py"])
    assert "python-tests" in verify.steps_for(["packages/emulator/control/src/lib.rs"])


def test_moved_paths_route() -> None:
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
            if path.startswith(
                "modules/"
            ):  # module-owned tests: none until a module has one
                continue
            assert verify.resolve_tests((path,)), (name, path)


def test_unmatched_entry_is_an_error() -> None:
    with pytest.raises(SystemExit):
        verify.resolve_tests(("tests/test_no_such_*.py",))
    with pytest.raises(SystemExit):
        verify.resolve_tests(("tests/test_no_such_file.py",))


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


def test_python_tests_passed(tmp_path: Path) -> None:
    head = "file\tstatus\trc\tseconds\tpassed\tfailed\tskipped\terrors\n"
    (tmp_path / "summary.tsv").write_text(head + "a.py\tOK\t0\t1.0\t0\t0\t3\t0\n")
    assert verify.python_tests_passed(tmp_path)
    (tmp_path / "summary.tsv").write_text(head + "a.py\tOK\t0\t1.0\t2\t0\t3\t0\n")
    assert verify.python_tests_passed(tmp_path) is None


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


def test_load_last_result(tmp_path: Path) -> None:
    assert verify.load_last_result(tmp_path / "none", None) is None
    older = tmp_path / "20260101-000000"
    newer = tmp_path / "20260102-000000"
    (tmp_path / "20260103-000000").mkdir()  # no result.json: skipped
    older.mkdir()
    newer.mkdir()
    (older / "result.json").write_text('{"n": 1}')
    (newer / "result.json").write_text('{"n": 2}')
    assert verify.load_last_result(tmp_path, None) == {"n": 2}
    assert verify.load_last_result(tmp_path, newer) == {"n": 1}
    # a result of another worktree is skipped
    (older / "result.json").write_text(
        json.dumps({"root": str(verify.ROOT), "failed": ["pins"]})
    )
    (newer / "result.json").write_text(
        json.dumps({"root": "/elsewhere", "failed": ["web"]})
    )
    assert verify.load_last_result(tmp_path, None) == {
        "root": str(verify.ROOT),
        "failed": ["pins"],
    }
    (newer / "result.json").write_text(json.dumps({"failed": ["web"]}))
    assert verify.load_last_result(tmp_path, None) == {"failed": ["web"]}


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


def test_explain_runs_nothing(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def fail(*args: object, **kwargs: object) -> None:
        raise AssertionError("a run directory was created")

    monkeypatch.setattr(verify.digi_env, "run_dir", fail)
    assert verify.main(["--explain", "--only", "pins"]) == 0
    assert "pins" in capsys.readouterr().out


# ---- finer routing (tools/devtools/verify_route.py) ----------------------------------------------

PCM_STEPS = {
    "build",
    "desktop",
    "pins",
    "check",
    "check-no-aot",
    "tier-quick",
    "tier-sweep",
    "dt2-tone",
    "dt2-check",
    "dt2-tier-quick",
    "dt2-tier-sweep",
    "card",
}


def mpath(mid: str) -> str:
    """The repo-relative directory of a module."""
    return modroots.module_dir(mid).relative_to(ROOT).as_posix()


def selected(*files: str) -> set[str]:
    """The python-tests paths beyond the always-on gate files."""
    return set(verify.python_tests(list(files), False)) - set(verify.PY_GATE)


def test_mise_new_task_routes_like_its_tool() -> None:
    old = '[tasks.a]\nrun = "echo"\n'
    task = '[tasks."dk:run"]\nrun = "uv run python -m tools.device.dkrun"\ndescription = "x"\n'
    got = verify_route.mise_paths(old, old + task)
    assert got == (
        ["tools/device/dkrun.py"],
        {"tools/device/dkrun.py": "mise.toml task dk:run"},
    )
    assert verify.steps_for(got[0]) == ("runtime-fixture", "python-tests", "smoke")
    # only the description or the alias changed: the same tool
    again = task.replace('description = "x"', 'description = "y"')
    assert verify_route.mise_paths(old + task, old + again) == got
    # an unchanged task selects nothing; a removed one only the path check
    assert verify_route.mise_paths(old + task, old + task) == ([], {})
    assert verify_route.mise_paths(old + task, old) == ([], {})


@pytest.mark.parametrize(
    ("old", "new"),
    [
        ('[tools]\nrust = "1"\n', '[tools]\nrust = "2"\n'),
        ('[env]\nA = "1"\n', '[env]\nA = "2"\n'),
        ('[tasks.verify]\nrun = "x"\n', '[tasks.verify]\nrun = "y"\n'),
        ('[tasks."verify:pre"]\nrun = "x"\n', '[tasks."verify:pre"]\nrun = "y"\n'),
        ('[tasks."gen:core"]\nrun = "x"\n', '[tasks."gen:core"]\nrun = "y"\n'),
        ('[tasks.build]\nrun = "x"\n', '[tasks.build]\nrun = "y"\n'),
        ("", '[tasks.t]\nrun = "cargo build"\n'),  # a run that names no file
        (
            "",
            '[tasks.t]\nrun = "uv run python -m tools.device.dkrun"\nenv = {A = "1"}\n',
        ),
        ("", "[tasks.t"),  # unreadable
    ],
)
def test_mise_broad_changes_stay_broad(old: str, new: str) -> None:
    assert verify_route.mise_paths(old, new) is None


def test_mise_missing_side_is_broad() -> None:
    assert verify_route.mise_paths(None, "") is None
    assert verify_route.mise_paths("", None) is None


def test_mise_tasks_of_the_repo_resolve() -> None:
    """Every task of the real mise.toml names a repo file or is listed here as broad: a new task
    that routes nowhere would make every edit of it run all steps, so it is listed on purpose."""
    import tomllib

    tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
    broad = set()
    for name, task in tasks.items():
        group = name.split(":", 1)[0]
        if group in verify_route.BROAD_TASK_GROUPS or "run" not in task:
            continue
        if verify_route.paths_of_run(verify_route.run_lines(task)) is None:
            broad.add(name)
    assert broad == {
        "dev:plusdrive",
        "lint:py",
    }  # cargo run; ruff on the whole include set


def test_expand_mise_replaces_a_task_change(monkeypatch: pytest.MonkeyPatch) -> None:
    old = '[tasks.a]\nrun = "echo"\n'
    new = old + '[tasks."dk:run"]\nrun = "uv run python -m tools.device.dkrun"\n'
    monkeypatch.setattr(verify, "merge_base", lambda base, head: "BASE")
    monkeypatch.setattr(
        verify, "git_show", lambda rev, path: old if rev == "BASE" else new
    )
    files, origin = verify.expand_mise(["mise.toml", "docs/x.md"], "b", None)
    assert files == ["docs/x.md", "tools/device/dkrun.py"]
    assert origin == {"tools/device/dkrun.py": "mise.toml task dk:run"}
    # a broad change keeps mise.toml, which selects every step
    monkeypatch.setattr(
        verify,
        "git_show",
        lambda rev, path: old if rev == "BASE" else old + "[env]\nA = 1\n",
    )
    files, _ = verify.expand_mise(["mise.toml"], "b", None)
    assert files == ["mise.toml"]
    assert verify.steps_for(files) == verify.EVERYTHING
    # the other files are untouched
    assert verify.expand_mise(["tools/x.py"], "b", None) == (["tools/x.py"], {})


def test_device_tools_select_their_tests_only() -> None:
    assert verify.steps_for(["tools/device/dkscreen.py"]) == ("python-tests", "smoke")
    assert selected("tools/device/dkscreen.py") == {
        "tests/digikit/test_dkscreen.py",
        "tests/digikit/test_upload_dn2.py",
        "tests/digikit/test_upload_dt2.py",
    }
    assert selected("tools/device/dkrun.py") == {
        "tests/digikit/test_dkrun.py",
        "tests/digikit/test_dktransfer.py",  # dktransfer imports dkrun
        "tests/digikit/test_menu_dn2.py",
        "tests/digikit/test_menu_dt2.py",
    }
    assert selected("tools/device/mmon.py") == set()
    assert "tests/patchkit" not in selected("tools/device/dkupload.py")
    assert "tests/digikit" not in selected("tools/device/dkupload.py")


def test_device_tools_nothing_dropped() -> None:
    """A change in a tools/device module runs every unit test that imports it, directly or through
    another tools/device module, and the scenario kinds whose runners call it."""
    for path in sorted((ROOT / "tools/device").glob("*.py")):
        stem = path.stem
        assert stem in verify_route.DEVICE_KINDS, f"classify tools/device/{stem}.py"
        got = selected(f"tools/device/{stem}.py")
        for test in sorted((ROOT / "tests").rglob("test_*.py")):
            if "node_modules" in test.parts:
                continue
            text = test.read_text()
            imports = rf"^\s*(from tools\.device import [^\n]*\b{stem}\b|from tools\.device\.{stem} import|import tools\.device\.{stem}\b)"
            if re.search(imports, text, re.M):
                assert test.relative_to(ROOT).as_posix() in got, (stem, test.name)
        for kind in verify_route.DEVICE_KINDS[stem]:
            assert set(verify_route.kind_files()[kind]) <= got, (stem, kind)
    kinds = set(verify_route.kind_files())
    for stem, wanted in verify_route.DEVICE_KINDS.items():
        assert set(wanted) <= kinds, stem


def test_digikit_test_files_and_scenarios() -> None:
    assert selected("tests/digikit/test_dkrun.py") == {"tests/digikit/test_dkrun.py"}
    assert selected("tests/patchkit/test_pk_cf.py") == {"tests/patchkit/test_pk_cf.py"}
    for device in ("dn2", "dt2"):
        deleted = f"tests/digikit/test_live_{device}.py"
        assert verify_route.digikit_fine(deleted) is None
        assert deleted not in verify.python_tests([deleted], False)
        assert "python-tests" in verify.steps_for([deleted])
    assert selected("tests/digikit/scenarios/menu-dkrun-pluck.toml") == {
        "tests/digikit/test_menu_dn2.py",
        "tests/digikit/test_menu_dt2.py",
    }
    assert verify.steps_for(["tests/digikit/scenarios/menu-dkrun-pluck.toml"]) == (
        "python-tests",
        "smoke",
    )
    # a helper of the suite: the test files that import it
    helper = selected("tests/digikit/scen_cases.py")
    assert "tests/digikit/test_menu_dt2.py" in helper
    assert "tests/digikit" not in helper
    # every scenario kind that a test file runs maps to a test file of tests/digikit
    for scen, (kind, _) in verify_route.scenario_refs().items():
        got = selected(scen)
        assert set(verify_route.kind_files().get(kind, ())) <= got, scen


def test_module_leaf_selects_the_tests_that_name_it() -> None:
    src = "modules/machines/bd/src/module.c"
    assert verify.steps_for([src]) == ("python-tests", "rust-tests", "smoke")
    # no test file names bd: the machine conformance suite and the test files of the scenario (equiv-dyn-bd) that names it
    assert selected(src) == {
        "tests/modules/test_machine_conformance.py",
        "tests/digikit/test_equiv_dt2.py",
        "tests/digikit/test_equiv_dn2.py",
    }
    # a module that tests name selects those tests, not whole directories
    named = selected("modules/fx/freqshift/src/module.c")
    assert "tests/patchkit/test_freqshift.py" in named
    assert "tests/patchkit/test_pk_cf.py" not in named
    assert "tests/patchkit" not in named
    # the object and the manifest are read by the web gallery
    assert "web" in verify.steps_for(["modules/machines/bd/module.toml"])
    assert "web" in verify.steps_for(["modules/machines/bd/build/bd-cf.dko.json"])
    assert "web" not in verify.steps_for(["modules/core/runtime/sdk/src/lib.rs"])
    assert verify.steps_for(["modules/core/runtime/README.md"]) == ()


def test_module_scenario_modules_select_their_scenarios() -> None:
    got = selected("modules/machines/pluck/module.toml")
    for name in (
        "test_equiv_dn2",
        "test_equiv_dt2",
        "test_menu_dn2",
        "test_menu_dt2",
        "test_roundtrip",
        "test_safe_cut_dn2",
        "test_safe_cut_dt2",
        "test_upload_dn2",
        "test_upload_dt2",
    ):
        assert f"tests/digikit/{name}.py" in got, name
    assert "tests/patchkit/test_patchkit_dkrt_machine.py" in got
    # digikit/pluck is the source of test/patched-pluck's DSP: the patched-in twin's tests follow
    assert "tests/digikit/test_menu_dt2.py" in selected(
        "modules/machines/pluck/src/dsp/pluck.s"
    )
    assert "tests/patchkit/test_pluck.py" in selected(
        "modules/machines/pluck/src/dsp/pluck.s"
    )


CONFORMANCE = "tests/modules/test_machine_conformance.py"


@pytest.mark.parametrize(
    "path",
    [
        "modules/machines/pluck/module.toml",
        "modules/machines/pluck/src/dsp/pluck.s",
        "tests/fixtures/modules/sine/src/dsp/sine.s",
        "tests/fixtures/modules/sine/module.toml",
        "modules/core/adapters/dsp-voice/src/dsp/dsp-voice-dn2.s",
    ],
)
def test_a_machine_change_selects_the_conformance_suite(path: str) -> None:
    assert CONFORMANCE in selected(path)
    assert "python-tests" in verify.steps_for([path])


@pytest.mark.parametrize(
    "path",
    ["modules/tools/remote/module.toml", "modules/core/adapters/params/module.toml"],
)
def test_other_modules_do_not_select_the_conformance_suite(path: str) -> None:
    assert CONFORMANCE not in selected(path)


@pytest.mark.parametrize(
    "path",
    [
        "tools/patchkit/pk_conform.py",
        "tools/patchkit/pk_sharc.py",
        "tests/support/machine_rig.py",
        "tests/support/conform_worker.py",
    ],
)
def test_the_conformance_machinery_selects_the_conformance_suite(path: str) -> None:
    assert verify_route.CONFORMANCE in selected(path)


@pytest.mark.parametrize(
    "path",
    [
        "tests/fixtures/modules/fx-gain/src/dsp/fx_gain.s",
        "tests/fixtures/modules/fx-gain/module.toml",
        "modules/core/adapters/fx/src/dsp/fx.s",
        "modules/core/adapters/fx/module.toml",
        "tools/patchkit/pk_conform.py",
        "tests/support/fx_rig.py",
        "tests/support/fx_conform_worker.py",
    ],
)
def test_an_effect_change_selects_the_fx_conformance_suite(path: str) -> None:
    assert verify_route.FX_CONFORMANCE in selected(path)


@pytest.mark.parametrize(
    "path",
    ["modules/machines/pluck/module.toml", "modules/core/adapters/params/module.toml"],
)
def test_other_modules_do_not_select_the_fx_conformance_suite(path: str) -> None:
    assert verify_route.FX_CONFORMANCE not in selected(path)


@pytest.mark.parametrize("module", sorted(verify_route.FRAMEWORK))
def test_framework_module_selects_both_suites(module: str) -> None:
    path = f"{mpath(module)}/module.toml"
    assert {"tests/patchkit", "tests/digikit"} <= selected(path)
    assert {"python-tests", "rust-tests"} <= set(verify.steps_for([path]))
    assert not set(verify.steps_for([path])) & PCM_STEPS


def test_module_changes_select_no_pcm_step() -> None:
    for module in verify_route.manifests():
        steps = verify.steps_for([f"{mpath(module)}/module.toml"])
        assert not set(steps) & PCM_STEPS, module


def test_module_change_drops_no_test_that_names_a_dependent() -> None:
    """For every module, the selection holds each test file that names it or a module that
    depends on it, and the test files of each scenario that does. The text scan here is
    independent of verify_route's (it reads comments too, so it can only find more)."""
    mods = verify_route.manifests()
    scenarios = sorted((ROOT / "tests/digikit/scenarios").glob("*.toml"))
    for mid in mods:
        got = selected(f"{mpath(mid)}/module.toml") | set(verify.PY_GATE)
        if "tests/patchkit" in got:
            continue
        hit = verify_route.dependents({mid})
        for test in sorted((ROOT / "tests").rglob("test_*.py")):
            if "node_modules" in test.parts or "devtools" in test.parts:
                continue
            if any(m in test.read_text() for m in hit):
                assert test.relative_to(ROOT).as_posix() in got, (mid, test.name)
        for scen in scenarios:
            if any(m in scen.read_text() for m in hit):
                rel = scen.relative_to(ROOT).as_posix()
                assert set(selected(rel)) <= got, (mid, scen.name)


def test_dependents_follow_requires_and_shared_sources() -> None:
    assert "test/patched-pluck" in verify_route.dependents({"core/machines"})
    # test/dyn-params builds from the DSP source of test/sine; the patched-in test/patched-pluck has its own copy
    assert "test/dyn-params" in verify_route.dependents({"test/sine"})
    assert verify_route.dependents({"digikit/pluck"}) == {"digikit/pluck"}
    assert verify_route.dependents({"fadedlimes/bd"}) == {"fadedlimes/bd"}


def test_unknown_module_path_stays_broad() -> None:
    steps = verify.steps_for(["modules/no/such/module.toml"])
    assert {"python-tests", "rust-tests", "web"} <= set(steps)
    assert {"tests/patchkit", "tests/digikit"} <= selected(
        "modules/no/such/module.toml"
    )


@pytest.mark.parametrize(
    "path",
    [
        "packages/dk-session/src/dkupload-core.ts",
        "packages/dk-session/src/gen_proto.ts",
        "packages/dk-session/package.json",
    ],
)
def test_dk_session_routes_to_host_with_conservative_smoke(
    path: str,
) -> None:
    expected = (
        ("host", "web", "runtime-fixture", "module-load", "smoke")
        if "/src/" in path
        else ("host", "web", "smoke")
    )
    assert verify.steps_for([path]) == expected


def test_root_host_config_routes_to_host_with_conservative_smoke() -> None:
    assert verify.steps_for(["vitest.host.config.ts"]) == ("host", "smoke")


def test_web_check_only_runs_browser_checks(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: dict[str, list[str]] = {}
    monkeypatch.setattr(
        verify.Gate,
        "run",
        lambda self, name, cmd, **kwargs: calls.update({name: cmd}),
    )
    monkeypatch.setattr(verify.Gate, "share", lambda self: 3)
    verify.web_check(object.__new__(verify.Gate))
    assert set(calls) == {"patchkit-wasm", "web-tests", "web-types"}
    assert calls["web-tests"][1] == "--test-concurrency=3"


def test_host_and_app_changes_keep_both_checks() -> None:
    assert verify.steps_for(
        ["packages/control-client/src/client.ts", "apps/web/src/components/X.tsx"]
    ) == ("host", "web", "stock", "runtime-fixture", "module-load", "smoke")


@pytest.mark.parametrize(
    "path",
    [
        "packages/control-client/package.json",
        "packages/test-support/tsconfig.json",
    ],
)
def test_host_paths_do_not_select_browser_checks(path: str) -> None:
    assert verify.steps_for([path]) == ("host", "smoke")


def test_web_only_for_what_it_reads() -> None:
    assert "web" in verify.steps_for(["apps/web/src/x.ts"])
    assert "web" in verify.steps_for(["packages/dkupload/src/lib.rs"])
    assert "rust-tests" in verify.steps_for(["packages/dkupload/src/lib.rs"])
    for path in (
        "tools/device/dkrun.py",
        "tests/digikit/scenarios/upload-quick.toml",
        "tools/patchkit/pk_digikit_scen.py",
        "tests/fixtures/modules/dyn-demo/src/x.s",
    ):
        assert "web" not in verify.steps_for([path]), path


def test_routing_changes_select_everything() -> None:
    for path in ("tools/devtools/verify.py", "tools/devtools/verify_route.py"):
        assert verify.steps_for([path]) == verify.EVERYTHING
        got = verify.python_tests([path], False)
        assert set(verify.python_tests([], True)) <= set(got)
    # the lane helper is checked by the routing tests only
    assert verify.steps_for(["tools/devtools/changed_tests.py"]) == (
        "python-tests",
        "smoke",
    )
    assert "tests/devtools/test_verify.py" in verify.python_tests(
        ["tools/devtools/changed_tests.py"], False
    )


def test_gate_files_always_run() -> None:
    for path in (
        "modules/machines/bd/src/module.c",
        "tools/device/mmon.py",
        "tests/digikit/scenarios/upload-quick.toml",
    ):
        assert set(verify.PY_GATE) <= set(verify.python_tests([path], False)), path


def test_runtime4_merge_runs_no_pcm_step() -> None:
    """The range 457f515b..29086ff7 (one mise.toml task, device tools, runtime objects) ran all 13
    steps; its files now select web, Rust and Python tests, without PCM steps."""
    files = [
        "modules/core/apps/digikit-menu/build/digikit-menu.dko.json",
        "modules/core/runtime/build/runtime.dko.json",
        "modules/core/runtime/src/rs/src/upload.rs",
        "tests/fixtures/modules/dyn-demo/build/dyn-demo.dko.json",
        "tests/digikit/scenarios/menu-dkrun-pluck.toml",
        "tests/digikit/test_dkrun.py",
        "tools/device/dkrun.py",  # the mise.toml dk:run task
        "tools/patchkit/pk_digikit_scen.py",
        "tools/patchkit/pk_digikit_upload.py",
    ]
    assert verify.steps_for(files) == (
        "host",
        "web",
        "runtime-fixture",
        "module-load",
        "python-tests",
        "rust-tests",
        "smoke",
    )


def test_rust_scope(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: list[list[str]] = []

    def fake(changed: list[str]) -> list[str] | None:
        seen.append(changed)
        return ["patchkit"]

    monkeypatch.setattr(verify.rust_ws, "affected_packages", fake, raising=False)
    assert verify.rust_scope(["modules/machines/bd/module.toml"]) == ["patchkit"]
    assert seen == [["modules/machines/bd/module.toml"]]
    # a broad mise.toml change and a routing change keep the whole workspace
    assert verify.rust_scope(["mise.toml"]) is None
    assert verify.rust_scope(["tools/devtools/verify.py"]) is None
    assert len(seen) == 1


def test_head_only_with_explain() -> None:
    with pytest.raises(SystemExit) as info:
        verify.main(["--head", "HEAD"])
    assert info.value.code == 2


def test_rust_tests_leaves_the_desktop_crate_to_the_desktop_step() -> None:
    from tools.devtools import rust_ws

    assert "digiemu-desktop" in rust_ws.affected_packages(
        ["packages/emulator/control/src/lib.rs"]
    )
    assert "--exclude" in rust_ws.workspace_args("test")
    assert "digiemu-desktop" in rust_ws.workspace_args("test")
    # a routed run whose crates include the app (or the whole workspace) gets the desktop step
    assert verify.with_desktop(
        ("rust-tests",), ["digi-control", "digiemu-desktop"]
    ) == ("desktop", "rust-tests")
    assert verify.with_desktop(("rust-tests",), None) == ("desktop", "rust-tests")
    assert verify.with_desktop(("rust-tests",), ["periph"]) == ("rust-tests",)
    assert verify.with_desktop(("python-tests",), None) == ("python-tests",)
    assert verify.with_desktop(("desktop", "rust-tests"), None) == (
        "desktop",
        "rust-tests",
    )


# ---- smoke step ------------------------------------------------------------------------------------


def test_smoke_runs_with_any_gate_step_not_for_docs() -> None:
    assert "smoke" in verify.steps_for(["tools/device/dkscreen.py"])
    assert "smoke" in verify.steps_for(["packages/emulator/machine/src/lib.rs"])
    assert "smoke" in verify.EVERYTHING
    assert "smoke" not in verify.steps_for(["docs/x.md"])
    assert verify.steps_for(["AGENTS.md"]) == ()


def test_smoke_waits_for_build_daemons_and_runs_after_it() -> None:
    nodes, deps = verify.plan(("smoke",))
    assert deps["smoke"] == ("build-daemons",)
    assert nodes.index("build-daemons") < nodes.index("smoke")
    assert "build" not in nodes
    assert "smoke" in verify.DAEMON_NODES


def test_python_daemon_tests_keep_their_daemon_prerequisite_without_smoke() -> None:
    _, deps = verify.plan(("python-tests",), ["tests/digikit/test_dkrun.py"])
    assert deps["python-tests"] == ("build-daemons",)


def test_smoke_alone_with_only(capsys: pytest.CaptureFixture[str]) -> None:
    assert verify.main(["--explain", "--only", "smoke"]) == 0
    out = capsys.readouterr().out
    assert "smoke" in out
    assert "pins" not in out


@pytest.mark.parametrize(
    "path, expected",
    [
        (
            "packages/test-support/src/session.ts",
            {"stock", "runtime-fixture", "module-load"},
        ),
        (
            "packages/control-client/src/client.ts",
            {"stock", "runtime-fixture", "module-load"},
        ),
        ("tests/regression/support.ts", {"stock", "runtime-fixture", "module-load"}),
        ("tests/regression/runtime/remote-input.test.ts", {"runtime-fixture"}),
        (
            "tests/regression/vitest.config.ts",
            {"stock", "runtime-fixture", "module-load"},
        ),
        ("tests/regression/dt2/transport.test.ts", {"stock"}),
        ("tests/regression/host/daemon.test.ts", set()),
        (
            "modules/core/runtime/src/rs/src/upload.rs",
            {"runtime-fixture", "module-load"},
        ),
        ("modules/machines/pluck/src/dsp/pluck.s", {"module-load"}),
        ("modules/machines/bd/src/module.c", set()),
        ("modules/no/such/module.toml", {"runtime-fixture", "module-load"}),
        ("tools/device/dkupload.py", {"runtime-fixture"}),
        ("tools/device/dkscreen.py", set()),
        ("tools/patchkit/freqshift_ref.py", set()),
        ("devices/digitakt-ii-leds.toml", set()),
        ("devices/digitone-ii-leds.toml", set()),
        ("tools/patchkit/patchkit.py", {"runtime-fixture", "module-load"}),
        ("tools/devtools/stablebin.py", {"runtime-fixture", "module-load"}),
        ("tools/emulator/card_overlay.py", {"runtime-fixture"}),
        ("tools/emulator/fast_regions.json", {"runtime-fixture", "module-load"}),
        (
            "packages/emulator/control/src/runtime_patch.rs",
            {"stock", "runtime-fixture", "module-load"},
        ),
        ("tools/devtools/verify.py", {"stock", "runtime-fixture", "module-load"}),
    ],
)
def test_relevant_native_acceptance_retains_existing_owners(
    path: str, expected: set[str]
) -> None:
    steps = set(verify.steps_for([path]))
    assert steps & {"stock", "runtime-fixture", "module-load"} == expected
    assert set(verify.base_route(path)[1]) <= steps


@pytest.mark.parametrize(
    "step, path, expected",
    [
        (
            "module-load",
            "modules/machines/pluck/src/dsp/pluck.s",
            ["modules/pluck.test.ts"],
        ),
        (
            "module-load",
            "modules/fx/freqshift/module.toml",
            ["modules/freqshift.test.ts"],
        ),
        ("module-load", "modules/tools/remote/module.toml", ["modules/remote.test.ts"]),
        ("module-load", "modules/machines/bd/module.toml", []),
        (
            "module-load",
            "tests/regression/modules/pluck.test.ts",
            ["modules/pluck.test.ts"],
        ),
        (
            "runtime-fixture",
            "tests/regression/runtime/remote-input.test.ts",
            ["runtime/remote-input.test.ts"],
        ),
        (
            "runtime-fixture",
            "modules/tools/remote/module.toml",
            ["runtime/remote-input.test.ts"],
        ),
        ("runtime-fixture", "modules/machines/bd/module.toml", []),
        ("stock", "tests/regression/dt2/transport.test.ts", ["dt2/transport.test.ts"]),
    ],
)
def test_acceptance_filters_native_files(
    step: str, path: str, expected: list[str]
) -> None:
    assert verify_route.acceptance_files(step, [path]) == expected


def test_broad_change_is_not_narrowed_by_a_module_leaf() -> None:
    paths = ["tools/devtools/verify.py", "modules/machines/pluck/module.toml"]
    assert verify.acceptance_plan("module-load", paths) == []  # all native module files


@pytest.mark.parametrize("name", ["samples", "dksample", "rpcfiles"])
def test_sample_clients_select_sample_acceptance_without_module_uploads(
    name: str,
) -> None:
    path = f"packages/dk-session/src/{name}.ts"
    assert verify_route.acceptance_files("runtime-fixture", [path]) == [
        "runtime/samples-live.test.ts",
        "runtime/samples-store-live.test.ts",
    ]
    assert "module-load" not in verify.steps_for([path])
    assert {"host", "web", "runtime-fixture"} <= set(verify.steps_for([path]))


@pytest.mark.parametrize("step", ["runtime-fixture", "module-load"])
@pytest.mark.parametrize(
    "path",
    [
        "modules/core/runtime/module.toml",
        "sdk/include/dk/gen/machine.h",
        "modules/no/such/module.toml",
    ],
)
def test_shared_abi_and_unknown_modules_keep_cross_device_acceptance(
    step: str, path: str
) -> None:
    files = verify_route.acceptance_files(step, [path])
    if step == "module-load":
        assert set(files) == {
            "modules/pluck.test.ts",
            "modules/freqshift.test.ts",
            "modules/remote.test.ts",
        }
    else:
        assert "runtime/remote-input.test.ts" in files
        assert "runtime/input-adapter.test.ts" in files


def test_changed_tests_reports_acceptance_gate_additions() -> None:
    sel = changed_tests.select(["tests/regression/runtime/transfer-live.test.ts"])
    assert any("runtime-fixture" in line for line in sel.gate_adds)


@pytest.mark.parametrize(
    "selected, packed",
    [
        ([], ["test/input-probe", "digikit/remote", "test/svc-echo"]),
        (["runtime/remote-input.test.ts"], ["digikit/remote"]),
        (["runtime/service-commands.test.ts"], ["test/svc-echo"]),
        (["runtime/input-adapter.test.ts"], ["test/input-probe"]),
        (["runtime/transfer-live.test.ts", "runtime/samples-live.test.ts"], []),
    ],
)
def test_runtime_fixture_packs_only_modules_of_selected_files(
    selected: list[str], packed: list[str]
) -> None:
    assert verify.runtime_pack_modules(selected) == packed


@pytest.mark.parametrize("flags", [["--only", "runtime-fixture"], ["--full"]])
def test_runtime_fixture_full_or_explicit_planning(
    flags: list[str],
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setenv("DIGIKIT_TIER", "quick")
    assert "runtime-fixture" in verify.STEPS
    assert verify.main(["--explain", *flags]) == 0
    out = capsys.readouterr().out
    steps = next(line for line in out.splitlines() if line.startswith("steps: "))
    assert "runtime-fixture" in steps
    if "--only" in flags:
        assert steps == "steps: runtime-fixture"
        assert "build-daemons" not in out


def test_runtime_fixture_explicit_plan_never_builds_but_waits_for_existing_build() -> (
    None
):
    nodes, deps = verify.plan(("runtime-fixture",))
    assert nodes == ["runtime-fixture"]
    assert deps["runtime-fixture"] == ()
    nodes, deps = verify.plan(("runtime-fixture", "smoke"))
    assert "build-daemons" in nodes
    assert deps["runtime-fixture"] == ("build-daemons",)


def test_a_failed_module_build_stops_before_pack_or_acceptance(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    gate = verify.Gate(
        tmp_path, budget=2, acceptance={"module-load": ["modules/pluck.test.ts"]}
    )
    calls = []

    def fail(name, cmd, **kw):
        calls.append(cmd)
        gate.failed.append(name)

    monkeypatch.setattr(gate, "run", fail)
    with pytest.raises(RuntimeError, match="module build objects"):
        verify.run_step(gate, "module-load")
    assert len(calls) == 1  # the build of the one module; no pack ran after it


def test_runtime_fixture_host_paths_do_not_select_device_acceptance() -> None:
    assert verify.steps_for(["tests/regression/host/runtime.test.ts"]) == ("host",)
    assert "runtime-fixture" not in verify.steps_for(
        ["tests/regression/host/remote-helpers.test.ts"]
    )


def test_runtime_fixture_rejects_insufficient_budget(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(verify.Gate, "step_env", lambda self: {"DIGI_JOBS": "1"})
    monkeypatch.setattr(verify.Gate, "share", lambda self: 1)
    with pytest.raises(RuntimeError, match="at least two cores"):
        verify.run_step(object.__new__(verify.Gate), "runtime-fixture")


@pytest.mark.parametrize("from_build", [False, True])
def test_runtime_fixture_dispatch_preserves_environment_and_built_paths(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    from_build: bool,
) -> None:
    monkeypatch.setenv("PATH", "/host/tool/bin")
    monkeypatch.setenv("DIGI_EMUD_DT2", "/supplied/dt2-emud")
    monkeypatch.setenv("DIGI_EMUD_DN2", "/supplied/dn2-emud")
    monkeypatch.setattr(verify.verify_budget, "machine_cargo_cap", lambda: 1)
    monkeypatch.setattr(verify.benchlock, "hold", lambda *_args: nullcontext())
    environments: list[dict[str, str]] = []

    def capture(
        cmd: list[str], *, env: dict[str, str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        if cmd[-3:] == ["-m", "tools.firmware.plusdrive_check", "--build-only"]:
            return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
        assert cmd == [
            "pnpm",
            "--filter",
            "@digi/regression",
            "test:runtime",
            "runtime/samples-live.test.ts",
        ]
        environments.append(env)
        return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

    monkeypatch.setattr(verify.subprocess, "run", capture)
    gate = verify.Gate(
        tmp_path,
        budget=2,
        acceptance={"runtime-fixture": ["runtime/samples-live.test.ts"]},
    )
    gate.local.node = "runtime-fixture"
    if from_build:
        gate.shared_env.update(
            {"DIGI_EMUD_DT2": "/built/dt2-emud", "DIGI_EMUD_DN2": "/built/dn2-emud"}
        )
    verify.run_step(gate, "runtime-fixture")
    assert len(environments) == 1
    env = environments[0]
    assert env["PATH"] == "/host/tool/bin"
    prefix = "built" if from_build else "supplied"
    assert env["DIGI_EMUD_DT2"] == f"/{prefix}/dt2-emud"
    assert env["DIGI_EMUD_DN2"] == f"/{prefix}/dn2-emud"
    assert not gate.failed


@pytest.mark.parametrize("steps", [("host",), ("web",)])
def test_failed_native_host_workflow_persists_for_rerun_and_blocks_web(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, steps: tuple[str, ...]
) -> None:
    calls: list[list[str]] = []

    def fail_host(
        cmd: list[str], *, env: dict[str, str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        calls.append(cmd)
        assert env["HOST_TEST_ENV"] == "inherited"
        return subprocess.CompletedProcess(cmd, 1, stdout="", stderr="bad format")

    monkeypatch.setenv("HOST_TEST_ENV", "inherited")
    monkeypatch.setattr(verify.verify_budget, "machine_cargo_cap", lambda: 1)
    monkeypatch.setattr(verify.benchlock, "hold", lambda *_args: nullcontext())
    monkeypatch.setattr(verify.subprocess, "run", fail_host)
    gate = verify.Gate(tmp_path, budget=2)
    gate.py_tests = []
    verify.run_steps(gate, steps)
    assert calls == [["pnpm", "verify:host"]]
    assert (tmp_path / "host.log").read_text() == "bad format"
    assert gate.failed.count("host") == 1
    failed = [step for step in gate.failed if step in verify.STEPS]
    assert verify.rerun_plan({"failed": failed, "py_tests": []}) == (
        tuple(step for step in verify.STEPS if step in {"host", *steps}),
        [],
    )
    assert ("web" in gate.failed) == ("web" in steps)


DN2_FX_STEPS = {"host", "check", "check-no-aot", "tier-quick", "smoke"}
DT2_FX_STEPS = {"host", "dt2-check", "dt2-tier-quick", "python-tests", "smoke"}


@pytest.mark.parametrize(
    "path, wanted",
    [
        ("tests/regression/scenarios/dn2-trackfx.ts", DN2_FX_STEPS),
        ("tests/regression/scenarios/dn2-trackfx.test.ts", DN2_FX_STEPS),
        ("tests/regression/scenarios/dn2-sendfx.ts", DN2_FX_STEPS),
        ("tests/regression/scenarios/dn2-sendfx.test.ts", DN2_FX_STEPS),
        ("tests/regression/scenarios/dt2-trackfx.ts", DT2_FX_STEPS),
        ("tests/regression/scenarios/dt2-sendfx.ts", DT2_FX_STEPS),
        ("tests/regression/scenarios/dt2-fx.test.ts", DT2_FX_STEPS),
        ("tests/regression/scenarios/fx-support.ts", DN2_FX_STEPS | DT2_FX_STEPS),
        ("tests/regression/vitest.config.ts", DN2_FX_STEPS | DT2_FX_STEPS),
    ],
)
def test_stock_audio_sources_keep_device_oracles_and_tier_checks(
    path: str, wanted: set[str]
) -> None:
    assert wanted <= set(verify.steps_for([path]))
    if path != "tests/regression/vitest.config.ts":  # the one config feeds all projects
        assert "runtime-fixture" not in verify.steps_for([path])


@pytest.mark.parametrize(
    "full,all_no_aot", [(False, False), (True, False), (False, True), (True, True)]
)
def test_fx_gate_tiers_override_ambient_selection(monkeypatch, full, all_no_aot):
    from types import SimpleNamespace

    monkeypatch.setenv("DIGIKIT_TIER", "full" if full else "quick")
    monkeypatch.setenv("DIGI_FX_SWEEP", "1")
    monkeypatch.setenv("DIGI_FX_NO_AOT_SUBSET", "1")
    calls = []
    gate = SimpleNamespace(
        all_no_aot=all_no_aot,
        step_env=lambda: {"DIGI_JOBS": "2"},
        run=lambda *args, **kwargs: calls.append((args, kwargs)),
    )
    verify.run_step(gate, "check")
    assert calls[-1][0] == (
        "check-fx",
        ["pnpm", "--filter", "@digi/regression", "test:fx"],
    )
    assert len(calls) == 1
    env = calls[-1][1]["env"]
    assert env["DIGI_FX_SWEEP"] == str(int(full))
    assert env["DIGI_FX_NO_AOT_SUBSET"] == "0"
    assert env["PATH"] == os.environ["PATH"]
    calls.clear()
    verify.run_step(gate, "dt2-check")
    assert calls[-1][0] == (
        "dt2-check",
        ["pnpm", "--filter", "@digi/regression", "test:fx:dt2"],
    )
    env = calls[-1][1]["env"]
    assert env["DIGI_FX_SWEEP"] == str(int(full))
    assert env["DIGI_FX_NO_AOT_SUBSET"] == "0"
    assert env["PATH"] == os.environ["PATH"]
    calls.clear()
    verify.run_step(gate, "check-no-aot")
    assert len(calls) == 1
    env = calls[-1][1]["env"]
    assert env["DIGI_FX_SWEEP"] == str(int(full and all_no_aot))
    assert env["DIGI_FX_NO_AOT_SUBSET"] == str(int(not all_no_aot))
    assert all(
        env[key] == value
        for key, value in verify.no_aot_env().items()
        if key not in {"DIGI_FX_SWEEP", "DIGI_FX_NO_AOT_SUBSET"}
    )


def test_module_ids_come_from_the_manifests_not_a_list_of_authors() -> None:
    verify_route._id_patterns.cache_clear()
    text = 'run("test/dyn-demo"); ROOT / "tests" / "digikit" / "test_x.py"; test/no-such-module'
    assert verify_route.refs(text) == {"test/dyn-demo"}
    # a name that only starts like an id is no reference
    assert verify_route.refs("test/dyn-demo-stray2 test/dyn-demo_x") == set()


def test_a_modules_own_scenario_is_found_and_selected_by_a_change_in_the_module() -> (
    None
):
    scen = "modules/machines/bd/tests/scenarios/equiv-dyn-bd.toml"
    refs_ = verify_route.scenario_refs()
    assert refs_[scen][0] == "equiv"
    assert "fadedlimes/bd" in refs_[scen][1]
    # a change inside the module's tests/ runs python-tests only, with the test files of the kind
    fine = verify_route.fine(scen)
    assert fine is not None and fine.steps == ("python-tests",)
    assert "tests/digikit/test_equiv_dt2.py" in fine.tests
    # a change in the module's sources selects the same scenario tests
    src = verify_route.fine("modules/machines/bd/module.toml")
    assert src is not None and "tests/digikit/test_equiv_dt2.py" in src.tests


def test_module_owned_python_tests_are_found_by_globbing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    test = tmp_path / "modules/machines/m/tests/sub/test_m.py"
    test.parent.mkdir(parents=True)
    test.write_text("")
    (tmp_path / "modules/machines/m/tests/helper.py").write_text("")
    monkeypatch.setattr(verify_route, "ROOT", tmp_path)
    assert verify_route.module_test_files() == [
        "modules/machines/m/tests/sub/test_m.py"
    ]
    assert verify.is_module_test_dir("modules/machines/m/tests/sub/test_m.py")
    assert not verify.is_module_test_dir("modules/machines/m/src/test_m.py")
