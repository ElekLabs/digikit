"""The PGO profile key covers the emulation crates, not the desktop UI crate."""

import json
import os
import pathlib

import pytest

from tools.emulator import (
    digi_env,
    native_pgo,
)

PACKAGES = ["packages/emulator/sharc", "packages/emulator/machine"]


@pytest.fixture
def tree(tmp_path, monkeypatch):
    """A tiny repo with a trained profile manifest for it."""
    for rel, text in {
        "packages/emulator/sharc/lib.rs": "a",
        "packages/emulator/machine/lib.rs": "b",
        "apps/desktop/src-tauri/main.rs": "ui",
        "Cargo.toml": "[workspace]",
        "Cargo.lock": "lock",
        "gen/core.rs": "core",
    }.items():
        path = tmp_path / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    state = tmp_path / "state"
    state.mkdir()
    toolchain = tmp_path / "toolchain"
    toolchain.mkdir()
    monkeypatch.setattr(native_pgo, "ROOT", tmp_path)
    monkeypatch.setattr(
        native_pgo, "state_dir", lambda triple=None, device="dn2": state
    )
    monkeypatch.setattr(native_pgo, "toolchain_dir", lambda: toolchain)
    monkeypatch.setattr(native_pgo, "rustc_version", lambda: "rustc 1.98.1")
    profdata = state / "p.profdata"
    profdata.write_bytes(b"profile")
    hasher = native_pgo.FileHasher(state / "hash-cache.json")
    components, _ = native_pgo.compute_components(PACKAGES, tmp_path / "gen", hasher)
    manifest = {
        "components": components,
        "packages": PACKAGES,
        "desktop_sha256": native_pgo.desktop_digest(hasher),
        "profdata": {"path": str(profdata)},
    }
    (state / "manifest.json").write_text(json.dumps(manifest))
    return tmp_path


def state(tree):
    return native_pgo.check_state(tree / "gen", "")


def test_sources_plus_generated_change_is_stale(tree):
    (tree / "packages/emulator/sharc/lib.rs").write_text("a changed")
    (tree / "gen/core.rs").write_text("new core")
    status, detail, _ = state(tree)
    assert status == "stale"
    assert "sources" in detail and "generated" in detail


def trained_here(tree, name="lane.profdata"):
    """Write by-key/<key>.json for the tree as it is now, with its own profile file."""
    state_dir = tree / "state"
    hasher = native_pgo.FileHasher(state_dir / "hash-cache.json")
    components, _ = native_pgo.compute_components(PACKAGES, tree / "gen", hasher)
    profdata = state_dir / name
    profdata.write_bytes(b"lane profile")
    manifest = {
        "key": native_pgo.key_of(components),
        "components": components,
        "packages": PACKAGES,
        "desktop_sha256": native_pgo.desktop_digest(hasher),
        "profdata": {"path": str(profdata)},
    }
    native_pgo.write_json_atomic(
        native_pgo.by_key_dir() / f"{manifest['key']}.json", manifest
    )
    return profdata


def test_exact_key_match_uses_its_profile_while_the_shared_manifest_is_partial(tree):
    (tree / "packages/emulator/sharc/lib.rs").write_text("a lane edit")
    assert state(tree)[0] == "partial"
    lane = trained_here(tree)
    assert state(tree) == ("ok", str(lane), False)
    assert native_pgo.current_profile(gen=tree / "gen") == str(lane)


def test_key_miss_falls_back_to_the_shared_manifest(tree):
    trained_here(tree)  # the key of the tree as trained
    (tree / "packages/emulator/sharc/lib.rs").write_text("another edit")
    status, detail, _ = state(tree)
    assert (status, detail) == ("partial", "changed: sources")
    assert native_pgo.current_profile(gen=tree / "gen") == str(
        tree / "state" / "p.profdata"
    )


def test_exact_match_without_its_profile_file_is_not_used(tree):
    (tree / "packages/emulator/sharc/lib.rs").write_text("a lane edit")
    trained_here(tree).unlink()
    assert state(tree)[0] == "partial"


# --- both: the two-core build -------------------------------------------------------


@pytest.fixture
def both(tree, monkeypatch):
    """The tree with a second generated core and a manifest trained for device `both`."""
    # The real shared out/ may hold a reset fixture; it must not change this workload.
    monkeypatch.setattr(native_pgo, "RESET_FIXTURE_DIR", tree / "no-reset-fixture")
    (tree / "gen2").mkdir()
    (tree / "gen2/core.rs").write_text("dt2 core")
    state_both = tree / "state-both"
    state_both.mkdir()
    monkeypatch.setattr(
        native_pgo,
        "state_dir",
        lambda triple=None, device="dn2": (
            state_both if device == "both" else tree / "state"
        ),
    )
    hasher = native_pgo.FileHasher(state_both / "hash-cache.json")
    components, _ = native_pgo.compute_components(
        PACKAGES, tree / "gen", hasher, gen2=tree / "gen2"
    )
    (state_both / "manifest.json").write_text(
        json.dumps(
            {
                "components": components,
                "packages": PACKAGES,
                "desktop_sha256": native_pgo.desktop_digest(hasher),
                "profdata": {"path": str(tree / "state" / "p.profdata")},
            }
        )
    )
    return tree


def both_state(tree):
    return native_pgo.check_state(tree / "gen", "", "", "both", tree / "gen2")


def test_both_key_covers_the_second_core_and_the_workload(tree):
    hasher = native_pgo.FileHasher(tree / "state" / "hash-cache.json")
    (tree / "gen2").mkdir()
    (tree / "gen2/core.rs").write_text("dt2 core")
    one, _ = native_pgo.compute_components(PACKAGES, tree / "gen", hasher)
    two, _ = native_pgo.compute_components(
        PACKAGES, tree / "gen", hasher, gen2=tree / "gen2"
    )
    assert "generated2" not in one
    assert two["generated2"] != two["generated"]
    assert two["features"] == one["features"]
    assert two["workload"] == "both-v1"
    assert two["devices"] == "generated=dn2,generated2=dt2"
    assert native_pgo.key_of(one) != native_pgo.key_of(two)


def test_both_profile_is_current_until_either_core_changes(both):
    assert both_state(both)[:2] == ("ok", str(both / "state" / "p.profdata"))
    (both / "gen2/core.rs").write_text("new dt2 core")
    assert both_state(both)[:2] == ("stale", "changed: generated2")
    (both / "gen2/core.rs").write_text("dt2 core")
    (both / "gen/core.rs").write_text("new dn2 core")
    assert both_state(both)[:2] == ("stale", "changed: generated")


def test_each_device_profile_builds_into_its_own_pgo_use_dir(tmp_path, monkeypatch):
    from tools.emulator import native_target

    gen = tmp_path / "dn2"
    gen.mkdir()
    (gen / "image.rs").write_text("")
    monkeypatch.setattr(native_target, "TARGET_ROOT", tmp_path / "target")
    monkeypatch.setattr(native_pgo, "toolchain_rustc", lambda: "/fake/rustc")
    env = {"SHARC_GEN_DIR": str(gen)}
    dirs = {
        device: native_pgo.plan_for("p", env, device).cargo_args[3]
        for device in ("dn2", "dt2", "both")
    }
    assert len(set(dirs.values())) == 3
    assert dirs["dn2"].endswith("/pgo-use")  # the launcher's reserved dir
    assert dirs["both"].endswith("/pgo-use-both")
    for device, target in dirs.items():
        assert str(native_pgo.pgo_binary(env, device)).startswith(target)


def test_each_device_sets_the_region_lists_of_its_own_engine_slot():
    env = {
        "SHARC_FAST_REGIONS": "0x1",
        "SHARC_BLOCKTRANS": "0x2",
        "NOTE_EVENTS": "trig",
    }
    assert digi_env.slot_env("dn2", env) == env
    assert digi_env.slot_env("dt2", env) == {
        "SHARC_FAST_REGIONS_2": "0x1",
        "SHARC_BLOCKTRANS_2": "0x2",
        "NOTE_EVENTS": "trig",
    }


def test_run_env_gives_a_dt2_run_the_lists_of_the_dt2_core(monkeypatch, tmp_path):
    from tools.devtools import core_store
    from tools.emulator import coupled

    monkeypatch.setattr(core_store, "library_env", lambda *a, **k: {})
    monkeypatch.setattr(digi_env, "fast_env", lambda env, syx=None, **k: {})
    syx = tmp_path / "dt2.syx"
    syx.write_bytes(b"x")
    monkeypatch.setattr(digi_env, "device_for_syx", lambda path: "dt2")
    monkeypatch.delenv("SHARC_FAST_REGIONS_2", raising=False)
    env = coupled.run_env(
        {"SHARC_FAST_REGIONS": "", "SHARC_BLOCKTRANS": "0x2"}, syx=syx
    )
    assert env["SHARC_FAST_REGIONS_2"] == "" and env["SHARC_BLOCKTRANS_2"] == "0x2"
    assert "SHARC_FAST_REGIONS" not in env and "SHARC_BLOCKTRANS" not in env
    monkeypatch.setattr(digi_env, "device_for_syx", lambda path: "dn2")
    env = coupled.run_env({"SHARC_BLOCKTRANS": "0x2"}, syx=syx)
    assert env["SHARC_BLOCKTRANS"] == "0x2" and "SHARC_BLOCKTRANS_2" not in env


# --- clean -------------------------------------------------------------------------------

NOW = 1_800_000_000.0
DAY = 86400


@pytest.fixture
def pgo(tmp_path):
    root = tmp_path / "pgo"
    state = root / "dn2" / "tri"
    (state / "profiles").mkdir(parents=True)
    (state / "by-key").mkdir()
    return root, state


def age(path, days):
    when = NOW - days * DAY
    os.utime(path, (when, when), follow_symlinks=False)


def manifest(state, name, where=None):
    where = where or state / "profiles"
    (state / "manifest.json").write_text(
        json.dumps({"profdata": {"path": str(where / f"{name}.profdata")}})
    )


def clean(pgo, days=14):
    root, state = pgo
    plan = native_pgo.plan_clean(state, days, now=NOW, root=root)
    native_pgo.apply_clean(plan, root=root)
    return plan


def test_clean_never_follows_a_symlink(pgo, tmp_path):
    root, state = pgo
    manifest(state, "m")
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "keep").write_text("x")
    age(outside / "keep", 90)
    (state / "bench-link").symlink_to(outside)
    age(state / "bench-link", 90)
    (state / "bench-real").mkdir()
    (state / "bench-real" / "inner").symlink_to(outside)
    age(state / "bench-real", 90)
    age(state / "bench-real" / "inner", 90)
    clean(pgo)
    assert (outside / "keep").exists()
    assert not (state / "bench-link").is_symlink()
    assert not (state / "bench-real").exists()


def test_clean_refuses_a_state_dir_outside_the_pgo_root(pgo, tmp_path):
    root, state = pgo
    other = tmp_path / "other"
    (other / "profiles").mkdir(parents=True)
    old = other / "bench-1"
    old.mkdir()
    age(old, 90)
    plan = native_pgo.plan_clean(other, 14, now=NOW, root=root)
    assert plan.items == [] and old.exists()


def test_not_current_notice_names_the_command_per_device():
    key = "ab" * 32
    assert "NOT current" in native_pgo.not_current_notice("dn2", key)
    assert native_pgo.not_current_notice("dn2", key).endswith(
        "native_pgo train --set-current"
    )
    assert native_pgo.not_current_notice("both", key).endswith(
        "native_pgo train --device both --set-current"
    )


@pytest.mark.parametrize(
    ("set_current", "at_main", "head", "origin", "expected"),
    [
        (True, False, "a", "b", True),
        (False, True, "a", "a", True),
        (False, True, "a", "b", False),
        (False, True, None, None, False),
        (False, False, "a", "a", False),
    ],
)
def test_decide_set_current(set_current, at_main, head, origin, expected):
    assert native_pgo.decide_set_current(set_current, at_main, head, origin) is expected


def record_training(tmp_path, monkeypatch, desktop="d1", fixtures=None, inputs=None):
    """A by-key manifest with its profile file, in a temp state dir."""
    state = tmp_path / "pgo-state"
    profile = state / "profiles" / "p.profdata"
    profile.parent.mkdir(parents=True)
    profile.write_bytes(b"profile")
    manifest = {
        "key": "k" * 64,
        "desktop_sha256": desktop,
        "inputs": {"regions": "r1"} if inputs is None else inputs,
        "profdata": {"path": str(profile)},
        **({"fixtures": fixtures} if fixtures is not None else {}),
    }
    (state / "by-key").mkdir()
    (state / "by-key" / f"{'k' * 64}.json").write_text(json.dumps(manifest))
    monkeypatch.setattr(native_pgo, "device_state", lambda device="dn2": state)
    saved = []
    monkeypatch.setattr(native_pgo, "save_manifest", lambda *args: saved.append(args))
    return profile, saved


INPUTS = {"regions": "r1"}


def current(device, key, force, set_current, inputs=INPUTS, **kwargs):
    return native_pgo.current_training(
        device, key, force, set_current, inputs, **kwargs
    )


def test_a_training_with_a_current_profile_is_skipped(tmp_path, monkeypatch):
    profile, saved = record_training(tmp_path, monkeypatch)
    assert current("dn2", "k" * 64, False, True, desktop="d1")
    assert saved and saved[0][2] is True  # --set-current still applies
    saved.clear()
    assert current("dn2", "k" * 64, False, False, desktop="d1")
    assert not saved  # a skip writes nothing without --set-current
    # --force, another key, another desktop digest, another input, a missing profile file: train
    assert not current("dn2", "k" * 64, True, False, desktop="d1")
    assert not current("dn2", "z" * 64, False, False, desktop="d1")
    assert not current("dn2", "k" * 64, False, False, desktop="d2")
    assert not current("dn2", "k" * 64, False, False, {"regions": "r2"}, desktop="d1")
    profile.unlink()
    assert not current("dn2", "k" * 64, False, False, desktop="d1")


def test_a_manifest_without_recorded_inputs_is_not_current(tmp_path, monkeypatch):
    record_training(tmp_path, monkeypatch)
    manifest_path = next((tmp_path / "pgo-state" / "by-key").iterdir())
    manifest = json.loads(manifest_path.read_text())
    del manifest["inputs"]
    manifest_path.write_text(json.dumps(manifest))
    assert not current("dn2", "k" * 64, False, False, desktop="d1")


def test_fixture_content_that_changed_trains_again(tmp_path, monkeypatch):
    record_training(
        tmp_path,
        monkeypatch,
        fixtures={"dn2": {"sha256": "a", "directory": "/lane-a/fx"}},
    )
    key = "k" * 64
    # the same content in another lane's directory is current
    assert current(
        "both",
        key,
        False,
        False,
        fixtures={"dn2": {"sha256": "a", "directory": "/lane-b/fx"}},
    )
    assert not current("both", key, False, False, fixtures={"dn2": {"sha256": "b"}})


def test_training_inputs_follow_workload_ready_inputs_and_regions(
    tmp_path, monkeypatch
):
    from tools.emulator import dt2_sample_check

    files = {}
    for name in ("scen.json", "ready.bin", "card.img"):
        files[name] = tmp_path / name
        files[name].write_text(name)
    monkeypatch.setattr(native_pgo, "ROOT", tmp_path)
    monkeypatch.setattr(native_pgo, "DN2_WORKLOAD", [("scen.json", ["a"])])
    monkeypatch.setattr(native_pgo, "DT2_WORKLOAD", [("scen.json", ["b"])])
    monkeypatch.setattr(native_pgo, "REPLAYS", {"dn2": ("scen.json", "c", None)})
    monkeypatch.setattr(
        digi_env, "ready_inputs", lambda *a, **k: {"firmware": files["ready.bin"]}
    )
    monkeypatch.setattr(
        dt2_sample_check,
        "card_inputs",
        lambda: (
            {"firmware": files["ready.bin"]},
            ["--card-image", str(files["card.img"])],
        ),
    )

    def inputs(regions="r1", runs=1):
        hasher = native_pgo.FileHasher(tmp_path / "hash-cache.json")
        return native_pgo.training_inputs(native_pgo.BOTH, hasher, regions, runs)

    record_training(tmp_path, monkeypatch, inputs=inputs())
    key = "k" * 64
    assert current("both", key, False, False, inputs())
    files["scen.json"].write_text("a workload edit")
    assert not current("both", key, False, False, inputs())
    files["scen.json"].write_text("scen.json")
    assert current("both", key, False, False, inputs())
    files["ready.bin"].write_text("another ready state")
    assert not current("both", key, False, False, inputs())
    files["ready.bin"].write_text("ready.bin")
    files["card.img"].write_text("another card")
    assert not current("both", key, False, False, inputs())
    files["card.img"].write_text("card.img")
    assert not current("both", key, False, False, inputs("another region list"))
    assert not current(
        "both", key, False, False, inputs(runs=3)
    )  # --runs 1 does not satisfy --runs 3
    monkeypatch.setattr(native_pgo, "REPLAYS", {"dn2": ("scen.json", "c", 9.0)})
    assert not current("both", key, False, False, inputs())  # a replay setting changed


def test_a_profile_covers_the_libraries_of_its_training_target():
    assert native_pgo.profile_devices("dn2") == ("dn2",)
    assert native_pgo.profile_devices("dt2") == ("dt2",)
    assert native_pgo.profile_devices("both") == ("dn2", "dt2")
    plan = native_pgo.LaunchPlan([], [], {}, profdata="/p.profdata", device="both")
    assert native_pgo.library_profiles(plan) == {
        "dn2": pathlib.Path("/p.profdata"),
        "dt2": pathlib.Path("/p.profdata"),
    }
    # no plan, or no profile: the plain libraries
    assert native_pgo.library_profiles(None) == {}
    assert native_pgo.library_profiles(native_pgo.LaunchPlan([], [], {})) == {}


def test_fresh_profiles_prefer_the_both_profile_and_fall_back_to_the_device(
    monkeypatch,
):
    def launch_plan(environ, target):
        profiles = {"dn2": "/dn2.p", "both": None, "dt2": "/dt2.p"}
        path = profiles[target]
        return (native_pgo.LaunchPlan([], [], {}, path, target) if path else None), ""

    monkeypatch.setattr(native_pgo, "launch_plan", launch_plan)
    assert native_pgo.fresh_profiles(["dn2", "dt2"], {}) == {
        "dn2": pathlib.Path("/dn2.p"),
        "dt2": pathlib.Path("/dt2.p"),
    }
    monkeypatch.setattr(
        native_pgo,
        "launch_plan",
        lambda environ, target: (
            native_pgo.LaunchPlan([], [], {}, "/both.p", target)
            if target == "both"
            else None,
            "",
        ),
    )
    assert native_pgo.fresh_profiles(["dn2"], {}) == {"dn2": pathlib.Path("/both.p")}


def test_the_package_list_of_a_tree_is_computed_once_per_lock_state(
    tmp_path, monkeypatch
):
    (tmp_path / "Cargo.lock").write_text("lock 1")
    (tmp_path / "Cargo.toml").write_text("[workspace]")
    calls = []
    monkeypatch.setattr(native_pgo, "ROOT", tmp_path)
    monkeypatch.setattr(native_pgo, "_PACKAGES_MEMO", {})
    monkeypatch.setattr(
        native_pgo, "local_package_dirs", lambda: calls.append(1) or ["packages/a"]
    )
    assert native_pgo.tree_packages() == ["packages/a"]
    assert native_pgo.tree_packages() == ["packages/a"] and len(calls) == 1
    # a new process (no memo) reads dist/pgo-packages.json
    monkeypatch.setattr(native_pgo, "_PACKAGES_MEMO", {})
    assert native_pgo.tree_packages() == ["packages/a"] and len(calls) == 1
    # a lock edit recomputes it
    (tmp_path / "Cargo.lock").write_text("lock 2")
    monkeypatch.setattr(native_pgo, "_PACKAGES_MEMO", {})
    native_pgo.tree_packages()
    assert len(calls) == 2


def test_an_edit_of_native_pgo_changes_the_package_list_key(tmp_path, monkeypatch):
    (tmp_path / "Cargo.lock").write_text("lock")
    (tmp_path / "Cargo.toml").write_text("[workspace]")
    monkeypatch.setattr(native_pgo, "ROOT", tmp_path)
    source = tmp_path / "native_pgo.py"
    source.write_text("LIBRARY_CRATES = ('a',)")
    monkeypatch.setattr(native_pgo, "__file__", str(source))
    before = native_pgo._packages_key()
    source.write_text("LIBRARY_CRATES = ('a', 'b')")
    assert native_pgo._packages_key() != before


def test_resolve_falls_back_when_cargo_metadata_is_malformed(tmp_path, monkeypatch):
    class Hasher:
        def __init__(self, path):
            pass

        def save(self):
            pass

    def malformed():
        raise ValueError("bad json")

    monkeypatch.setattr(
        native_pgo, "read_manifest", lambda device: {"components": {}, "packages": []}
    )
    monkeypatch.setattr(native_pgo, "toolchain_dir", lambda: tmp_path)
    monkeypatch.setattr(native_pgo, "device_state", lambda device: tmp_path)
    monkeypatch.setattr(native_pgo, "FileHasher", Hasher)
    monkeypatch.setattr(native_pgo, "compute_components", lambda *a: ({}, 0))
    monkeypatch.setattr(native_pgo, "desktop_digest", lambda hasher: "d")
    monkeypatch.setattr(native_pgo, "read_by_key", lambda device, key: None)
    monkeypatch.setattr(
        native_pgo, "profdata_path", lambda manifest, device: tmp_path / "none.profdata"
    )
    monkeypatch.setattr(native_pgo, "tree_packages", malformed)
    result = native_pgo.resolve("dn2", gen=tmp_path)
    assert result.status == "missing"


def test_a_failed_cache_write_keeps_the_list_and_leaves_no_part_file(
    tmp_path, monkeypatch
):
    (tmp_path / "Cargo.lock").write_text("lock")
    (tmp_path / "Cargo.toml").write_text("[workspace]")
    monkeypatch.setattr(native_pgo, "ROOT", tmp_path)
    monkeypatch.setattr(native_pgo, "_PACKAGES_MEMO", {})
    monkeypatch.setattr(native_pgo, "local_package_dirs", lambda: ["packages/a"])

    def refuse(src, dst):
        raise OSError("read-only")

    monkeypatch.setattr(native_pgo.os, "replace", refuse)
    assert native_pgo.tree_packages() == ["packages/a"]
    assert list((tmp_path / "dist").glob("*.part")) == []
    assert not (tmp_path / "dist" / "pgo-packages.json").exists()


def test_desktop_digest_ignores_build_output_but_not_rust_source(tree):
    crate = tree / native_pgo.DESKTOP_DIR
    hasher = native_pgo.FileHasher(tree / "state" / "hash-cache.json")
    before = native_pgo.desktop_digest(hasher)
    for rel in ("gen/schemas/x.json", "icons/a.png", "project.json"):
        (crate / rel).parent.mkdir(parents=True, exist_ok=True)
        (crate / rel).write_text("rebuilt")
    assert native_pgo.desktop_digest(hasher) == before
    (crate / "main.rs").write_text("changed code")
    assert native_pgo.desktop_digest(hasher) != before


def test_a_dt2_profile_is_resolved_against_the_dt2_core_not_the_launchers(
    monkeypatch,
):
    monkeypatch.setattr(native_pgo, "supported_host", lambda: True)
    seen = {}

    def check(gen, fast, blocktrans, device, gen2):
        seen[device] = gen
        return "ok", "/p.profdata"

    monkeypatch.setattr(native_pgo, "check", check)
    monkeypatch.setattr(
        native_pgo, "second_generated", lambda environ=None: pathlib.Path("/dt2-core")
    )
    environ = {"SHARC_GEN_DIR": "/dn2-core"}
    native_pgo.pgo_state(environ, "dt2")
    native_pgo.pgo_state(environ, "dn2")
    assert seen == {"dt2": pathlib.Path("/dt2-core"), "dn2": pathlib.Path("/dn2-core")}


def test_the_dt2_core_override_wins_over_the_launcher_core():
    environ = {"SHARC_GEN_DIR": "/dn2-core", "SHARC_GEN_DIR_DT2": "/own"}
    assert native_pgo.device_generated("dt2", environ) == pathlib.Path("/own")
    assert native_pgo.device_generated("dn2", environ) == pathlib.Path("/dn2-core")


def test_an_instrumented_library_is_restored_by_key_and_built_when_missing(
    tmp_path, monkeypatch
):
    from tools.devtools import core_store, emud_key

    gen = tmp_path / "gen"
    gen.mkdir()
    (gen / "blocks_00.rs").write_text("fn a() {}")
    (gen / "image.rs").write_text('pub const IMAGE: &str = "dn2-1.12";\n')
    builds = []

    def build(plan, inputs, log=None):
        builds.append(log)
        lib = tmp_path / "built" / core_store.lib_name()
        lib.parent.mkdir(exist_ok=True)
        lib.write_bytes(b"library")
        return lib

    monkeypatch.setenv("DIGI_SHARC_PGO_INSTRUMENT_STORE", str(tmp_path / "store"))
    monkeypatch.setattr(digi_env, "check_generated", lambda gen: None)
    monkeypatch.setattr(emud_key, "rustc_version", lambda: "rustc 1")
    monkeypatch.setattr(core_store, "build_core", build)
    log = tmp_path / "lib.log"
    first = native_pgo.instrumented_library("dn2", gen, log)
    assert first.read_bytes() == b"library" and builds == [log]
    # the same key: restored, no second build
    assert native_pgo.instrumented_library("dn2", gen, log) == first
    assert len(builds) == 1
    # another gen file content: another key, built again
    (gen / "blocks_00.rs").write_text("fn b() {}")
    assert native_pgo.instrumented_library("dn2", gen, log) != first
    assert len(builds) == 2


def test_training_and_lookup_compute_the_same_dt2_key(monkeypatch, tmp_path):
    """SHARC_GEN_DIR names the launcher's DN2 core: the dt2 training (`dt2_generated`) and every lookup ignore it."""
    dt2_core = tmp_path / "dt2-core"
    monkeypatch.setattr(native_pgo, "second_generated", lambda environ=None: dt2_core)
    monkeypatch.setenv("SHARC_GEN_DIR", str(tmp_path / "dn2-core"))
    monkeypatch.delenv("SHARC_GEN_DIR_DT2", raising=False)
    seen = []

    def resolve(device, gen=None, gen2=None, *rest):
        seen.append(gen)
        return native_pgo.Resolved(None, "missing", "x")

    monkeypatch.setattr(native_pgo, "resolve", resolve)
    monkeypatch.setattr(native_pgo, "supported_host", lambda: True)
    assert native_pgo.dt2_generated() == dt2_core  # what cmd_train_dt2 keys with
    native_pgo.pgo_state({"SHARC_GEN_DIR": str(tmp_path / "dn2-core")}, "dt2")
    native_pgo.current_profile("dt2", native_pgo.device_generated("dt2"))
    assert seen == [dt2_core, dt2_core]
