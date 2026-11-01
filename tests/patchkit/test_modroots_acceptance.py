"""Project-owned acceptance metadata feeds existing routing and preparation."""

import json

import pytest

from tools.devtools import verify, verify_route
from tools.patchkit import modroots


@pytest.fixture
def owned(tmp_path, monkeypatch):
    directory = tmp_path / "modules/fx/effect"
    tests = directory / "tests"
    tests.mkdir(parents=True)
    (tests / "effect.device.test.ts").write_text("")
    (directory / "module.toml").write_text('[module]\nid="owned/effect"\n')
    fixture = tmp_path / "tests/fixtures/modules/source"
    fixture.mkdir(parents=True)
    oracle = tmp_path / "tools/oracle.py"
    oracle.parent.mkdir()
    oracle.write_text("")
    project = directory / "project.json"
    metadata = {
        "files": ["tests/effect.device.test.ts"],
        "modules": ["owned/effect", "test/source"],
        "sources": ["tools/oracle.py"],
        "plusdrive": True,
    }
    project.write_text(json.dumps({"metadata": {"runtimeAcceptance": metadata}}))
    monkeypatch.setattr(modroots, "ROOT", tmp_path)
    monkeypatch.setattr(
        modroots,
        "manifests",
        lambda: {"owned/effect": directory, "test/source": fixture},
    )
    monkeypatch.setattr(verify_route, "ROOT", tmp_path)
    monkeypatch.setattr(verify, "ROOT", tmp_path)
    monkeypatch.setattr(verify_route, "runtime_framework", lambda: set())
    monkeypatch.setattr(verify_route, "dependents", lambda ids: ids)
    return project, metadata


def test_registered_owned_and_fixture_changes_share_preparation(owned):
    _, metadata = owned
    repo_file = "modules/fx/effect/tests/effect.device.test.ts"
    normalized = "../../" + repo_file
    _, units = verify.named_units([], [repo_file])
    assert units == {"runtime-fixture": [normalized]}
    assert verify.runtime_pack_modules([normalized]) == metadata["modules"]
    assert normalized in verify_route.runtime_plusdrive_files()
    for source in [
        repo_file,
        "modules/fx/effect/module.toml",
        "tests/fixtures/modules/source/module.toml",
        "tools/oracle.py",
    ]:
        assert verify_route.acceptance_files("runtime-fixture", [source]) == [
            normalized
        ]
    with pytest.raises(ValueError):
        verify.named_units([], ["../../" + repo_file])
    with pytest.raises(ValueError):
        verify.named_units([], ["runtime/../../../" + repo_file])


def test_sources_may_be_empty_and_plusdrive_is_explicit(owned):
    project, metadata = owned
    metadata["sources"] = []
    metadata["plusdrive"] = False
    project.write_text(json.dumps({"metadata": {"runtimeAcceptance": metadata}}))
    assert modroots.runtime_acceptance()[0].sources == ()
    assert (
        "../../modules/fx/effect/tests/effect.device.test.ts"
        not in verify_route.runtime_plusdrive_files()
    )


@pytest.mark.parametrize(
    "field,value",
    [
        ("files", ["../outside.device.test.ts"]),
        ("modules", ["unknown/effect"]),
        ("sources", ["../outside.py"]),
        ("plusdrive", "yes"),
    ],
)
def test_invalid_ownership_fails_closed(owned, field, value):
    project, metadata = owned
    metadata[field] = value
    project.write_text(json.dumps({"metadata": {"runtimeAcceptance": metadata}}))
    with pytest.raises(ValueError, match="invalid runtimeAcceptance"):
        modroots.runtime_acceptance()


def test_unregistered_generic_device_file_fails_closed(owned):
    project, _ = owned
    (project.parent / "tests/extra.device.test.ts").write_text("")
    with pytest.raises(ValueError, match="unregistered"):
        modroots.runtime_acceptance()


def test_missing_registration_fails_closed(owned):
    project, _ = owned
    project.write_text("{}")
    with pytest.raises(ValueError, match="unregistered"):
        modroots.runtime_acceptance()


def test_deleted_declared_source_still_selects_consumer(owned):
    project, _ = owned
    (project.parents[3] / "tools/oracle.py").unlink()
    assert verify_route.acceptance_files("runtime-fixture", ["tools/oracle.py"]) == [
        "../../modules/fx/effect/tests/effect.device.test.ts"
    ]
