"""Runtime acceptance records of the Nx project graph feed routing and preparation, and fail closed."""

import pytest

from tools.devtools import verify_route


@pytest.fixture
def graph(tmp_path, monkeypatch):
    tests = tmp_path / "tests/regression/runtime"
    tests.mkdir(parents=True)
    (tests / "a.test.ts").write_text("")
    owned = tmp_path / "modules/fx/effect/tests"
    owned.mkdir(parents=True)
    (owned / "effect.device.test.ts").write_text("")
    metadata = {
        "files": ["runtime/a.test.ts"],
        "modules": ["test/source"],
        "aliases": {"test/source-2": "test/source"},
        "sources": ["tools/oracle.py"],
        "plusdrive": True,
    }
    nodes = {
        "regression": {
            "data": {
                "root": "tests/regression",
                "tags": [],
                "metadata": {"runtimeAcceptance": [metadata]},
            }
        },
        "owned/effect": {
            "data": {
                "root": "modules/fx/effect",
                "tags": ["module"],
                "metadata": {
                    "runtimeAcceptance": [{"files": ["tests/effect.device.test.ts"]}]
                },
            }
        },
        "test/source": {
            "data": {"root": "tests/fixtures/modules/source", "tags": ["module"]}
        },
    }
    monkeypatch.setattr(verify_route, "ROOT", tmp_path)
    monkeypatch.setattr(
        verify_route, "nx_graph", lambda: {"nodes": nodes, "dependencies": {}}
    )
    verify_route.manifests.cache_clear()
    verify_route.acceptance_records.cache_clear()
    yield metadata, nodes
    verify_route.manifests.cache_clear()
    verify_route.acceptance_records.cache_clear()


def test_records_map_files_to_owner_modules_and_aliases(graph):
    records = verify_route.acceptance_records()
    assert set(records) == {
        "runtime/a.test.ts",
        "../../modules/fx/effect/tests/effect.device.test.ts",
    }
    assert records["../../modules/fx/effect/tests/effect.device.test.ts"].modules == (
        "owned/effect",
    )
    assert verify_route.runtime_aliases() == {
        "test/source-2": ("test/source", ("runtime/a.test.ts",))
    }
    assert verify_route.runtime_plusdrive_files() == {"runtime/a.test.ts"}


@pytest.mark.parametrize(
    "field,value",
    [
        ("files", ["../outside.test.ts"]),
        ("files", ["runtime/missing.test.ts"]),
        ("modules", ["unknown/effect"]),
        ("aliases", {"test/x": "unknown/effect"}),
        ("sources", ["../outside.py"]),
        ("plusdrive", "yes"),
        ("extra", 1),
    ],
)
def test_invalid_record_fails_closed(graph, field, value):
    metadata, _ = graph
    metadata[field] = value
    with pytest.raises(ValueError, match="invalid runtimeAcceptance"):
        verify_route.acceptance_records()


def test_file_in_two_records_fails_closed(graph):
    _, nodes = graph
    nodes["regression"]["data"]["metadata"]["runtimeAcceptance"].append(
        {"files": ["runtime/a.test.ts"]}
    )
    with pytest.raises(ValueError, match="duplicate"):
        verify_route.acceptance_records()


def test_unregistered_device_file_fails_closed(graph, tmp_path):
    (tmp_path / "modules/fx/effect/tests/extra.device.test.ts").write_text("")
    with pytest.raises(ValueError, match="unregistered"):
        verify_route.acceptance_records()
