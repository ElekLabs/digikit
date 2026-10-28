"""tools/devtools/changed_tests.py: the test selection, without git or a subprocess."""

from __future__ import annotations

import ast
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import Mock

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.devtools import changed_tests, verify_route  # noqa: E402


def _no_directory(sel: changed_tests.Selection) -> None:
    for path in sel.tests:
        assert (ROOT / path).is_file(), path


def test_docs_select_nothing() -> None:
    assert changed_tests.select(["docs/TESTING.md"]).tests == {}


def test_patchkit_tool_selects_its_importer() -> None:
    sel = changed_tests.select(["tools/patchkit/pk_harness.py"])
    reason = sel.tests["tests/patchkit/smoke/test_pk_harness_units.py"]
    assert reason.startswith("imports")
    _no_directory(sel)


def test_framework_module_selects_conformance_not_a_directory() -> None:
    sel = changed_tests.select(["modules/core/adapters/dsp-voice/module.toml"])
    assert verify_route.CONFORMANCE in sel.tests
    _no_directory(sel)
    assert any(line.startswith("tests/patchkit") for line in sel.gate_adds)


def test_graph_links_support_imports() -> None:
    # tests import helpers as `support.x`; the graph links them to tests.support.x
    g = changed_tests.graph()
    assert "tests.support.paths" in g.find_modules_directly_imported_by(
        "tests.devtools.test_changed_tests"
    )


def _module_test_imports(text: str) -> set[str]:
    """The names a module test file imports."""
    out: set[str] = set()
    for n in ast.walk(ast.parse(text)):
        if isinstance(n, ast.ImportFrom) and n.level == 0 and n.module:
            out.add(n.module)
        elif isinstance(n, ast.Import):
            out |= {a.name for a in n.names}
    return out


def test_graph_links_module_tests_to_their_local_helpers_and_tools() -> None:
    # a file in modules/<kind>/<name>/tests/ imports a sibling by plain name, or a tools module
    g = changed_tests.graph()
    siblings = linked_tools = 0
    for f in changed_tests.module_test_files():
        mod = changed_tests.module_of(f)
        assert mod in g.modules, f
        here = (ROOT / f).parent
        for name in _module_test_imports((ROOT / f).read_text(encoding="utf-8")):
            if (here / f"{name}.py").is_file():
                assert f"{mod.rsplit('.', 1)[0]}.{name}" in (
                    g.find_modules_directly_imported_by(mod)
                ), (f, name)
                siblings += 1
            elif name.startswith("tools.") and name in g.modules:
                assert name in g.find_modules_directly_imported_by(mod), (f, name)
                linked_tools += 1
    assert siblings and linked_tools


def test_graph_links_a_module_run_by_name() -> None:
    # test_machine_conformance starts `-m support.conform_worker` in a subprocess
    g = changed_tests.graph()
    assert "tests.support.conform_worker" in g.find_modules_directly_imported_by(
        "tests.modules.test_machine_conformance"
    )


def test_graph_drops_imports_inside_functions_of_tools() -> None:
    # digi_env imports the patchkit front end only inside one function
    g = changed_tests.graph()
    assert "tools.patchkit.patchkit" not in g.find_modules_directly_imported_by(
        "tools.emulator.digi_env"
    )


def _module_tests_reaching(*markers: str) -> set[str]:
    """The module test files whose text, or the text of a sibling helper they import, contains one of MARKERS."""
    found: set[str] = set()
    for f in changed_tests.module_test_files():
        if not changed_tests.is_test_file(f):
            continue
        here = (ROOT / f).parent
        texts = [(ROOT / f).read_text(encoding="utf-8")]
        texts += [
            (here / f"{name}.py").read_text(encoding="utf-8")
            for name in _module_test_imports(texts[0])
            if (here / f"{name}.py").is_file()
        ]
        if any(m in t for m in markers for t in texts):
            found.add(f)
    return found


def test_pk_sharc_selects_the_routine_tests_and_no_sharc_semantics() -> None:
    tests, _ = changed_tests.gate_tests(["tools/patchkit/pk_sharc.py"])
    assert verify_route.CONFORMANCE in tests
    assert "tests/patchkit/test_sine.py" in tests
    # the module-owned tests that import pk_sharc, directly or through a local helper
    rigs = [
        f"support.{q.stem}"
        for q in (ROOT / "tests/support").glob("*.py")
        if "pk_sharc" in q.read_text(encoding="utf-8")
    ]
    reaching = _module_tests_reaching(*rigs)
    assert reaching and reaching <= set(tests)
    assert not any(t.startswith("tests/sharc/") for t in tests)


def test_a_shared_test_helper_selects_the_module_tests_that_import_it() -> None:
    tests, _ = changed_tests.gate_tests(["tests/support/machine_rig.py"])
    reaching = _module_tests_reaching("support.machine_rig")
    assert reaching and reaching <= set(tests)


def test_a_module_test_helper_selects_the_tests_that_import_it() -> None:
    helpers = [
        f
        for f in changed_tests.module_test_files()
        if not changed_tests.is_test_file(f) and "/scenarios/" not in f
    ]
    for helper in helpers:
        stem = Path(helper).stem
        users = {
            f
            for f in changed_tests.module_test_files()
            if changed_tests.is_test_file(f)
            and Path(f).parent == Path(helper).parent
            and stem in _module_test_imports((ROOT / f).read_text(encoding="utf-8"))
        }
        if users:
            tests, _ = changed_tests.gate_tests([helper])
            assert users <= set(tests), helper
            return
    pytest.fail("no module test helper imported by a module test")


LAUNCHER_PATHS = [
    f"tools/emulator/{name}.py"
    for name in ("coupled", "digi_env", "native_emu", "native_pgo")
]


@pytest.mark.parametrize("files", [[p] for p in LAUNCHER_PATHS] + [LAUNCHER_PATHS])
def test_bounded_launchers_keep_owners_consumers_and_full_smoke(
    files: list[str],
) -> None:
    tests, _ = changed_tests.gate_tests(files)
    for path in files:
        module = changed_tests.module_of(path)
        assert set(changed_tests.importing_tests(module)) <= set(tests)
    assert {
        "tests/devtools/test_verify.py",  # graph cycle downstream of scen_cases
        "tests/devtools/test_sharc_matrix.py",
        "tests/emulator/test_coupled_patched.py",
        "tests/patchkit/test_dyn_fx.py",
    } <= set(tests)
    assert not any(
        p.startswith("tests/digikit/") and Path(p).name.startswith("test_fx")
        for p in tests
    )
    assert "tests/digikit/test_menu_dt2.py" not in tests
    legacy = {
        changed_tests.path_of(m)
        for m in changed_tests.graph().find_downstream_modules(
            "tests.digikit.scen_cases"
        )
        if m.startswith("tests.digikit.test_")
    }
    assert not legacy & set(tests)
    plan = set(changed_tests.verify.python_tests(files, False))
    assert {
        "tests/emulator/test_coupled_tool.py",
        "tests/emulator/test_native_emu_launcher.py",
        "tests/emulator/test_native_pgo.py",
        "tests/emulator/test_native_target.py",
        "tests/emulator/test_gen_fingerprint.py",
    } <= plan
    assert {"build", "pins", "python-tests", "smoke"} <= set(
        changed_tests.verify.steps_for(files)
    )
    assert changed_tests.verify.smoke_cells(files) is None


def test_bounded_launcher_keeps_digikit_units() -> None:
    tests, _ = changed_tests.gate_tests(["tools/emulator/digi_env.py"])
    assert {"tests/digikit/test_dkrun.py", "tests/digikit/test_dktransfer.py"} <= set(
        tests
    )


def test_bounded_launcher_never_filters_direct_scenario_importers(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    importing = changed_tests.importing_tests
    scenario = "tests/digikit/test_fx_dt2.py"

    def with_scenario(module: str) -> dict[str, str]:
        return {**importing(module), scenario: "imports launcher directly"}

    monkeypatch.setattr(changed_tests, "importing_tests", with_scenario)
    tests, _ = changed_tests.gate_tests(LAUNCHER_PATHS)
    assert tests[scenario] == "imports launcher directly"


@pytest.mark.parametrize(
    "helper",
    [
        "tools/patchkit/pk_digikit_scen.py",
        "tools/patchkit/pk_dkrt.py",
        "tools/patchkit/pk_harness.py",
        "tests/digikit/scen_cases.py",
    ],
)
@pytest.mark.parametrize("launchers", [[], LAUNCHER_PATHS])
def test_scenario_helpers_restore_scenarios_per_source(
    helper: str, launchers: list[str]
) -> None:
    tests, _ = changed_tests.gate_tests([*launchers, helper])
    assert {
        "tests/digikit/test_fx_dt2.py",
        "tests/digikit/test_fx_dn2.py",
        "tests/digikit/test_menu_dt2.py",
        "tests/digikit/test_upload_dn2.py",
    } <= set(tests)


def test_explicit_scenario_and_full_suites_remain_selected() -> None:
    scenario = "tests/digikit/test_fx_dt2.py"
    assert scenario in changed_tests.verify.python_tests(
        [*LAUNCHER_PATHS, scenario], False
    )
    full = set(changed_tests.verify.python_tests(LAUNCHER_PATHS, True))
    for _, _, paths in changed_tests.verify.PY_SUITES:
        assert set(changed_tests.verify.resolve_tests(paths)) <= full


def test_neighboring_launcher_keeps_conservative_scenario_selection() -> None:
    tests, _ = changed_tests.gate_tests(["tools/emulator/native_target.py"])
    assert "tests/digikit/test_fx_dt2.py" in tests
    assert "tests/digikit/test_upload_dn2.py" in tests


def test_missing_scenario_helper_keeps_conservative_selection(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    graph = Mock(wraps=changed_tests.graph())
    graph.modules = set(changed_tests.graph().modules) - {"tests.digikit.scen_cases"}
    monkeypatch.setattr(changed_tests, "graph", lambda: graph)
    tests, _ = changed_tests.gate_tests(LAUNCHER_PATHS)
    assert "tests/digikit/test_fx_dt2.py" in tests
    assert "tests.digikit.scen_cases" not in {
        call.args[0] for call in graph.find_downstream_modules.call_args_list
    }


@pytest.mark.parametrize(
    "path",
    [
        "tools/devtools/coupled_ab.py",
        "tools/devtools/sharc_regionbench.py",
        "tools/devtools/sharc_matrix.py",
    ],
)
def test_benchmark_tools_have_no_legacy_scenario_fanout(path: str) -> None:
    tests, _ = changed_tests.gate_tests([path])
    assert not any(p.startswith("tests/digikit/") for p in tests)


def test_sharc_memory_keeps_native_acceptance_steps() -> None:
    path = "packages/emulator/sharc-rt/src/mem.rs"
    assert set(changed_tests.verify.steps_for([path])) == {
        "build",
        "desktop",
        "host",
        "web",
        "stock",
        "runtime-fixture",
        "module-load",
        "pins",
        "check",
        "check-no-aot",
        "tier-quick",
        "python-tests",
        "rust-tests",
        "card",
        "smoke",
        "dt2-tone",
        "dt2-check",
        "dt2-tier-quick",
    }
    assert changed_tests.verify.smoke_cells([path]) is None


@pytest.mark.parametrize(
    "path, own_reason",
    [
        ("tests/devtools/test_verify.py", "changed"),  # a changed test selects itself
        ("tests/support/machine_rig.py", None),  # a support module selects importers
    ],
)
def test_changed_test_or_support_module_selects_tests(
    path: str, own_reason: str | None
) -> None:
    sel = changed_tests.select([path])
    assert sel.tests
    if own_reason:
        assert sel.tests[path] == own_reason
    else:
        assert all(
            "via" in r or "imports" in r or "runs" in r for r in sel.tests.values()
        )


def test_selection_never_holds_a_directory() -> None:
    paths = sorted(
        p.relative_to(ROOT).as_posix()
        for sub in ("tools/devtools", "tools/patchkit")
        for p in (ROOT / sub).glob("*.py")
    )
    for path in paths:
        _no_directory(changed_tests.select([path]))


def test_deep_is_a_superset() -> None:
    path = "tools/emulator/digi_env.py"
    shallow = changed_tests.select([path])
    deep = changed_tests.select([path], deep=True)
    assert not deep.deep_only
    assert set(shallow.tests) <= set(deep.tests)


@pytest.mark.parametrize("returncodes", [(0, 0), (1, 0), (0, 1), (1, 1)])
def test_main_propagates_exit_codes_and_continues(
    returncodes: tuple[int, int],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    names = ["python-tests", "ruff-check"]
    monkeypatch.setattr(changed_tests.verify, "changed_files", lambda _: [])
    monkeypatch.setattr(changed_tests.verify, "rust_scope", lambda _: [])
    monkeypatch.setattr(
        changed_tests, "plan", lambda *args: [(n, [n], "") for n in names]
    )
    monkeypatch.setattr(changed_tests.digi_env, "run_dir", lambda _: str(tmp_path))
    monkeypatch.setattr(changed_tests.verify_budget, "resolve_budget", lambda: 1)
    monkeypatch.setattr(changed_tests.verify_budget, "bounded_env", lambda *args: {})
    run = Mock(
        side_effect=[
            CompletedProcess([n], rc) for n, rc in zip(names, returncodes, strict=True)
        ]
    )
    monkeypatch.setattr(changed_tests.subprocess, "run", run)

    assert changed_tests.main(["--foreground"]) == int(any(returncodes))
    assert [call.args[0] for call in run.call_args_list] == [[n] for n in names]
    for call in run.call_args_list:
        assert not {"capture_output", "stdout", "stderr", "text"} & call.kwargs.keys()
    output = capsys.readouterr().out
    assert f"python-tests logs: {tmp_path}" in output
    for name, rc in zip(names, returncodes, strict=True):
        assert f"{'FAIL' if rc else 'PASS'} {name} (" in output
    if any(returncodes):
        failed = ", ".join(n for n, rc in zip(names, returncodes, strict=True) if rc)
        assert f"test:changed: FAILED: {failed}." in output


def test_explain_runs_nothing(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(changed_tests.verify, "changed_files", lambda _: [])
    monkeypatch.setattr(changed_tests.verify, "rust_scope", lambda _: [])
    monkeypatch.setattr(
        changed_tests, "plan", lambda *args: [("python-tests", ["unused"], "")]
    )
    run = Mock(side_effect=AssertionError("explain must not run commands"))
    monkeypatch.setattr(changed_tests.subprocess, "run", run)
    assert changed_tests.main(["--explain"]) == 0
    run.assert_not_called()
    output = capsys.readouterr().out
    assert "Deferred to `mise run verify`" in output
    assert "RUN  python-tests: unused" in output
    assert "explained, nothing run" in output


def test_fast_plan_defers_module_and_native_work() -> None:
    files = [
        "modules/core/adapters/dsp-voice/module.toml",
        "packages/emulator/control/src/lib.rs",
        "tools/device/dkrun.py",
    ]
    commands = {n: c for n, c, _ in changed_tests.plan(files, Path("OUT"))}
    assert commands["ruff-check"][-1] == "tools/device/dkrun.py"
    assert commands["pathcheck"]
    assert "rust-tests" not in commands and "pk-build-check" not in commands
    tests = commands["python-tests"] or []
    assert not any(
        p.startswith("tests/") and p not in changed_tests.FAST_PYTHON_TESTS
        for p in tests
    )


def test_fast_python_tests_are_the_devtools_suite() -> None:
    suite = next(
        tests for name, _, tests in changed_tests.verify.PY_SUITES if name == "devtools"
    )
    assert {
        t for t in suite if t.startswith("tests/devtools/")
    } == changed_tests.FAST_PYTHON_TESTS
    assert "tests/devtools/test_verify.py" in changed_tests.FAST_PYTHON_TESTS


def test_fast_plan_runs_only_selected_audited_units(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        changed_tests,
        "select",
        lambda *args: changed_tests.Selection(
            tests={
                "tests/devtools/test_verify.py": "changed",
                "tests/patchkit/test_dyn_fx.py": "imports",
            }
        ),
    )
    commands = {n: c for n, c, _ in changed_tests.plan([], Path("OUT"))}
    assert "tests/devtools/test_verify.py" in commands["python-tests"]
    assert "tests/patchkit/test_dyn_fx.py" not in commands["python-tests"]


def test_host_selection_uses_native_related_for_sources_tests_and_assets() -> None:
    for path in (
        "packages/control-client/src/index.ts",
        "packages/control-client/tests/client.test.ts",
        "apps/desktop/ui/src/native.css",
    ):
        commands = {n: c for n, c, _ in changed_tests.host_plan([path])}
        related = commands["host-tests-related"]
        assert related[related.index("vitest") + 1] == "related"
        assert "--run" in related and path in related
        if path.endswith(".ts"):
            assert commands["types"] == ["pnpm", "-r", "run", "typecheck"]
        else:
            assert "types" not in commands
        assert not any(name.startswith("types-") for name in commands)
    for path in (
        "pnpm-lock.yaml",
        "vitest.host.config.ts",
        "packages/dk-session/package.json",
    ):
        commands = {n: c for n, c, _ in changed_tests.host_plan([path])}
        assert commands["host-tests"][1] == "test:host"


def test_deferred_output_names_python_crates_and_changed_files(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(changed_tests.verify, "python_test_plan", lambda *args: [])
    monkeypatch.setattr(changed_tests.verify, "rust_scope", lambda _: ["digi-control"])
    changed_tests.print_deferred(
        ["modules/core/adapters/dsp-voice/module.toml"],
        changed_tests.Selection(tests={"tests/patchkit/test_dyn_fx.py": "imports"}),
    )
    output = capsys.readouterr().out
    assert "Python: tests/patchkit/test_dyn_fx.py" in output
    assert "Rust crates: digi-control" in output
    assert "Full gate coverage: modules/core/adapters/dsp-voice/module.toml" in output


def test_unselected_audited_gate_units_are_still_deferred(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(changed_tests.verify, "python_test_plan", lambda *args: [])
    monkeypatch.setattr(changed_tests.verify, "rust_scope", lambda _: [])
    changed_tests.print_deferred(
        [],
        changed_tests.Selection(
            deep_only={"tests/devtools/test_verify_budget.py": "deep"}
        ),
    )
    assert "Python: tests/devtools/test_verify_budget.py" in capsys.readouterr().out
