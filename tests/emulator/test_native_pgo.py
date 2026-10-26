"""The PGO profile key covers the emulation crates, not the desktop UI crate."""

import hashlib
import json
import subprocess

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT
from tools.emulator import (  # noqa: E402
    digi_env,  # noqa: E402
    native_pgo,  # noqa: E402
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


def test_unchanged_tree_is_ok(tree):
    assert state(tree) == ("ok", str(tree / "state" / "p.profdata"), False)


def test_desktop_crate_change_reuses_the_profile(tree):
    (tree / "apps/desktop/src-tauri/main.rs").write_text("ui, diagnostics fix")
    status, _, desktop = state(tree)
    assert (status, desktop) == ("ok", True)
    assert native_pgo.check(tree / "gen", "")[0] == "ok"


def test_desktop_files_in_a_listed_package_list_are_ignored(tree):
    manifest_path = tree / "state" / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["packages"] = [*PACKAGES, "apps/desktop/src-tauri"]
    manifest_path.write_text(json.dumps(manifest))
    (tree / "apps/desktop/src-tauri/main.rs").write_text("changed")
    assert state(tree)[0] == "ok"


def test_emulation_crate_change_is_partly_stale(tree):
    (tree / "packages/emulator/sharc/lib.rs").write_text("a changed")
    status, detail, _ = state(tree)
    assert (status, detail) == ("partial", "changed: sources")


def test_rustc_change_is_stale(tree, monkeypatch):
    monkeypatch.setattr(native_pgo, "rustc_version", lambda: "rustc 1.99.0")
    assert state(tree)[:2] == ("stale", "changed: rustc")


def test_sources_plus_generated_change_is_stale(tree):
    (tree / "packages/emulator/sharc/lib.rs").write_text("a changed")
    (tree / "gen/core.rs").write_text("new core")
    status, detail, _ = state(tree)
    assert status == "stale"
    assert "sources" in detail and "generated" in detail


def test_generated_core_change_is_stale_but_fast_regions_are_not(tree):
    assert native_pgo.check_state(tree / "gen", "0x1")[0] == "ok"
    (tree / "gen/core.rs").write_text("new core")
    assert state(tree)[1] == "changed: generated"
    assert native_pgo.check_state(tree / "gen", "0x1")[1] == "changed: generated"


def test_blocktrans_roots_do_not_make_the_profile_stale(tree):
    assert native_pgo.check_state(tree / "gen", "", "0xb8276c")[0] == "ok"


def test_blocktrans_roots_follow_env_then_table(monkeypatch, tmp_path):
    syx = tmp_path / "fw.syx"
    syx.write_bytes(b"dn2")
    table = tmp_path / "fast.json"
    digest = hashlib.sha256(b"dn2").hexdigest()
    table.write_text(json.dumps({digest: {"blocktrans": "0x2"}}))
    monkeypatch.setattr(digi_env, "FAST_TABLE", table)
    monkeypatch.setenv("DIGI_COUPLED_SYX", str(syx))
    monkeypatch.delenv("DIGI_EMU_SYX", raising=False)
    digi_env._syx_digest.cache_clear()
    assert native_pgo.blocktrans_roots({"SHARC_BLOCKTRANS": "0x1"}) == "0x1"
    assert native_pgo.blocktrans_roots({"DIGI_EMU_FAST": "0"}) == ""
    assert native_pgo.blocktrans_roots({"DIGI_COUPLED_SYX": str(syx)}) == "0x2"
    assert native_pgo.blocktrans_roots() == "0x2"


def test_current_profile_returns_the_file_even_when_stale(tree):
    (tree / "packages/emulator/sharc/lib.rs").write_text("a changed")
    (tree / "gen/core.rs").write_text("new core")
    assert state(tree)[0] == "stale"
    assert native_pgo.current_profile() == str(tree / "state" / "p.profdata")
    (tree / "state" / "p.profdata").unlink()
    assert native_pgo.current_profile() is None
    (tree / "state" / "manifest.json").unlink()
    assert native_pgo.current_profile() is None


def test_missing_profile_file_and_manifest(tree):
    (tree / "state" / "p.profdata").unlink()
    assert state(tree)[0] == "missing"
    (tree / "state" / "manifest.json").unlink()
    assert state(tree) == ("missing", "no manifest", False)


def test_cmd_check_reports_the_reuse(tree, monkeypatch, capsys):
    monkeypatch.setattr(native_pgo, "fast_regions", lambda: "")
    monkeypatch.setattr(native_pgo, "blocktrans_roots", lambda: "")
    monkeypatch.setattr(native_pgo, "generated_dir", lambda environ=None: tree / "gen")
    (tree / "apps/desktop/src-tauri/main.rs").write_text("ui 2")
    assert native_pgo.cmd_check(None) == 0
    assert (
        capsys.readouterr().out.strip() == "ok (desktop crate changed; profile reused)"
    )


def test_local_package_dirs_excludes_apps(monkeypatch):
    metadata = {
        "packages": [
            {"source": None, "manifest_path": str(ROOT / name / "Cargo.toml")}
            for name in (
                "packages/emulator/sharc",
                "packages/emulator/coldfire",
                "apps/desktop/src-tauri",
            )
        ]
        + [{"source": "registry+x", "manifest_path": "/elsewhere/Cargo.toml"}]
    }
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda *a, **k: subprocess.CompletedProcess(a, 0, json.dumps(metadata), ""),
    )
    assert native_pgo.local_package_dirs() == [
        "packages/emulator/coldfire",
        "packages/emulator/sharc",
    ]


def test_launch_plan_skips_a_stale_profile_with_one_line(tree, monkeypatch):
    monkeypatch.setattr(native_pgo, "supported_host", lambda: True)
    monkeypatch.setattr(native_pgo, "toolchain_rustc", lambda: "/fake/rustc")
    monkeypatch.setattr(native_pgo, "generated_dir", lambda environ=None: tree / "gen")
    (tree / "gen/core.rs").write_text("new core")
    plan, notice = native_pgo.launch_plan({})
    assert plan is None
    assert "stale for this core; building without PGO" in notice
    assert "mise run perf:pgo" in notice
    assert "-no-pgo-warn-mismatch" in native_pgo.profile_use_flags("p")


def test_launch_plan_uses_a_partly_stale_profile(tree, monkeypatch):
    monkeypatch.setattr(native_pgo, "supported_host", lambda: True)
    monkeypatch.setattr(native_pgo, "toolchain_rustc", lambda: "/fake/rustc")
    monkeypatch.setattr(native_pgo, "generated_dir", lambda environ=None: tree / "gen")
    monkeypatch.setattr(
        native_pgo,
        "native_target",
        lambda: type("T", (), {"target_dir": lambda *a, **k: tree / "target"}),
    )
    (tree / "packages/emulator/sharc/lib.rs").write_text("a changed")
    plan, notice = native_pgo.launch_plan({})
    assert plan is not None
    assert "-no-pgo-warn-mismatch" in plan.env["RUSTFLAGS"]
    assert str(tree / "state" / "p.profdata") in plan.env["RUSTFLAGS"]
    assert notice == (
        "PGO profile partly stale (sources changed since training); using it. "
        "Retrain at milestones with `mise run perf:pgo`."
    )


def test_dt2_state_reads_the_dt2_manifest(tree, monkeypatch):
    dt2 = tree / "state-dt2"
    dt2.mkdir()
    monkeypatch.setattr(
        native_pgo,
        "state_dir",
        lambda triple=None, device="dn2": tree / "state" if device == "dn2" else dt2,
    )
    assert native_pgo.check_state(tree / "gen", "", "", "dt2")[0] == "missing"
    manifest = json.loads((tree / "state" / "manifest.json").read_text())
    # The DT2 key carries the DT2 workload name, so a DN2 manifest is stale.
    (dt2 / "manifest.json").write_text(json.dumps(manifest))
    assert native_pgo.check_state(tree / "gen", "", "", "dt2")[1] == "changed: workload"
    manifest["components"]["workload"] = native_pgo.DT2_WORKLOAD_NAME
    (dt2 / "manifest.json").write_text(json.dumps(manifest))
    assert native_pgo.check_state(tree / "gen", "", "", "dt2")[0] == "ok"
    assert native_pgo.current_profile("dt2") == str(tree / "state" / "p.profdata")


# --- crate-hash guard -------------------------------------------------------------

PROFILE_TEXT = """Counters:
  _RNvNtCsAAA111_12sharc_native6engine3run:
  _RNvCsBBB222_9sharc_gen4core:
  _RNvCsCCC333_8sharc_rt3mem:
  elektron_native_boot.1234-cgu.03;_RNvCsDDD444_20elektron_native_boot4boot:
  _RNvNtCsOTHER_4core3ptr9drop_glue:
  _RNvCsEEE555_12sharc_native3dup:
"""


def test_crates_in_maps_crates_to_disambiguators():
    assert native_pgo.crates_in(PROFILE_TEXT) == {
        "sharc_native": {"AAA111", "EEE555"},
        "sharc_gen": {"BBB222"},
        "sharc_rt": {"CCC333"},
        "elektron_native_boot": {"DDD444"},
    }


def test_profile_crates_parses_and_caches(tmp_path, monkeypatch):
    profdata = tmp_path / "p.profdata"
    profdata.write_text("x")
    runs = []

    def fake_run(command, **_kwargs):
        runs.append(command)
        return subprocess.CompletedProcess(command, 0, stdout=PROFILE_TEXT)

    monkeypatch.setattr(subprocess, "run", fake_run)
    first = native_pgo.profile_crates(profdata)
    assert first["sharc_gen"] == {"BBB222"}
    assert (tmp_path / "p.profdata.crates.json").is_file()
    assert native_pgo.profile_crates(profdata) == first
    assert len(runs) == 1


def test_binary_crates_reads_nm_output(monkeypatch):
    nm = "0000 T _RNvCsZZZ999_12sharc_native3run\n0001 t _RNvCsZZZ999_8sharc_rt3x\n"
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda command, **_kw: subprocess.CompletedProcess(command, 0, stdout=nm),
    )
    assert native_pgo.binary_crates("bin") == {
        "sharc_native": {"ZZZ999"},
        "sharc_rt": {"ZZZ999"},
    }


def test_dt2_app_ignores_its_own_boot_crate():
    profile = {"sharc_native": {"A"}, "elektron_native_boot": {"B"}}
    binary = {"sharc_native": {"A"}, "elektron_native_boot": {"C"}}
    assert native_pgo.crate_mismatches(profile, binary)
    ignore = native_pgo.DT2_APP_UNPROFILED
    assert native_pgo.crate_mismatches(profile, binary, ignore) == []
    binary["sharc_native"] = {"Z"}
    assert native_pgo.crate_mismatches(profile, binary, ignore)


def test_crate_mismatches_reports_only_disjoint_crates():
    profile = native_pgo.crates_in(PROFILE_TEXT)
    same = {k: set(v) for k, v in profile.items()}
    assert native_pgo.crate_mismatches(profile, same) == []
    other = {**same, "sharc_gen": {"NEW"}}
    assert native_pgo.crate_mismatches(profile, other) == [
        "MISMATCH crate sharc_gen: profile ['BBB222'] binary ['NEW']"
    ]


def test_fallback_regions_follow_explicit_fast_optout_and_table(monkeypatch, tmp_path):
    syx = tmp_path / "fw.syx"
    syx.write_bytes(b"fw")
    table = tmp_path / "fast.json"
    table.write_text(
        json.dumps({hashlib.sha256(b"fw").hexdigest(): {"fallback_regions": "0x2"}})
    )
    monkeypatch.setattr(digi_env, "FAST_TABLE", table)
    base = {"DIGI_EMU_SYX": str(syx)}
    assert native_pgo.fallback_regions(base) == "0x2"
    assert native_pgo.fallback_regions({**base, "SHARC_FAST_REGIONS": ""}) == ""
    assert native_pgo.fallback_regions({**base, "DIGI_EMU_FAST": "0"}) == ""
    assert native_pgo.fallback_regions({**base, "SHARC_FAST_AUTO": "10,100"}) == ""
    assert (
        native_pgo.fallback_regions(
            {**base, "SHARC_FAST_AUTO": "10,100", "SHARC_FAST_FALLBACK_REGIONS": "0x3"}
        )
        == "0x3"
    )
    assert (
        native_pgo.fallback_regions(
            {**base, "SHARC_FAST_REGIONS": "", "SHARC_FAST_FALLBACK_REGIONS": "0x3"}
        )
        == "0x3"
    )
    assert (
        native_pgo.fallback_regions({**base, "SHARC_FAST_FALLBACK_REGIONS": ""}) == ""
    )


def test_fallback_configuration_reports_change_without_invalidating_profile(
    tree, monkeypatch, capsys
):
    monkeypatch.setenv("SHARC_FAST_FALLBACK_REGIONS", "0x2")
    monkeypatch.setattr(native_pgo, "fast_regions", lambda: "")
    monkeypatch.setattr(native_pgo, "blocktrans_roots", lambda: "")
    monkeypatch.setattr(native_pgo, "generated_dir", lambda environ=None: tree / "gen")
    assert state(tree)[0] == "ok"
    assert native_pgo.cmd_check(None) == 0
    assert (
        "fast regions changed since training; the profile still applies"
        in capsys.readouterr().out
    )
