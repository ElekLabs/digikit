"""How verify_route selects acceptance files for a changed tools/ Python path (no Nx, no device)."""

from __future__ import annotations

import pytest

from tools.devtools import verify_route
from tools.devtools.verify_route import RuntimeAcceptance

A, B, C = "runtime/a.test.ts", "runtime/b.test.ts", "runtime/c.test.ts"
SHARED = "tools/patchkit/patchkit.py"
# Existing files: a path that is not on disk counts as deleted.
ONLY_A = "tools/devtools/verify_budget.py"
DECLARED = "tools/devtools/stablebin.py"
UNKNOWN = "tools/devtools/verify.py"


def record(file: str, *sources: str) -> RuntimeAcceptance:
    return RuntimeAcceptance("tests/regression", (file,), (), {}, sources, False)


@pytest.fixture(autouse=True)
def fake_routing(monkeypatch: pytest.MonkeyPatch) -> None:
    a = record(A, DECLARED)
    records = {A: a, B: record(B), C: record(C)}
    closures = {a: {DECLARED, ONLY_A}}
    monkeypatch.setattr(verify_route, "acceptance_records", lambda: records)
    monkeypatch.setattr(verify_route, "acceptance_shared", lambda step: {SHARED})
    monkeypatch.setattr(
        verify_route, "acceptance_record_closure", lambda r: closures.get(r, set())
    )


def select(step: str, path: str) -> set[str]:
    return set(verify_route.acceptance_files(step, [path], resolve_helpers=False))


def runtime_files() -> set[str]:
    return set(verify_route.acceptance_records())


def test_shared_closure_path_selects_all_runtime_files() -> None:
    assert runtime_files() <= select("runtime-fixture", SHARED)


def test_path_reached_from_one_record_selects_its_files() -> None:
    assert select("runtime-fixture", ONLY_A) == {A}


def test_declared_source_selects_its_records() -> None:
    assert select("runtime-fixture", DECLARED) == {A}


def test_deleted_tools_path_selects_all_runtime_files() -> None:
    assert runtime_files() <= select("runtime-fixture", "tools/patchkit/deleted_x.py")


def test_path_in_no_closure_selects_nothing() -> None:
    assert select("runtime-fixture", UNKNOWN) == set()


def test_module_load_uses_only_the_shared_rule() -> None:
    assert select("module-load", ONLY_A) == set()
    assert select("module-load", "tools/patchkit/deleted_x.py") == set()
    assert select("module-load", SHARED)
