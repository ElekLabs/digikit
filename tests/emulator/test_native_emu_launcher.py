"""The launcher restores local DN2 audio by default, with CF-only escape hatches."""

import json
import os
import runpy
import subprocess
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT
from tools.emulator import digi_env  # noqa: E402
from tools.firmware import device as device_data  # noqa: E402


@pytest.fixture(autouse=True)
def pinned_rustc(monkeypatch):
    """Never ask rustup for the toolchain from these tests."""
    monkeypatch.setattr(digi_env, "toolchain_rustc", lambda: "/fake/bin/rustc")


@pytest.fixture(autouse=True)
def fresh_gen(monkeypatch):
    """The fake gen dirs here carry no CORE_FINGERPRINT."""
    monkeypatch.setenv("DIGI_ALLOW_STALE_GEN", "1")


@pytest.fixture(autouse=True)
def known_device(monkeypatch):
    """The fake firmware files of these tests belong to a device with a DSP; a test of another device sets its own."""
    short = [d.short for d in device_data.load_all() if d.has_dsp()][-1]
    monkeypatch.setattr(digi_env, "device_for_syx", lambda _path: short)


@pytest.fixture(autouse=True)
def no_plus_drive_folder(monkeypatch, tmp_path_factory):
    """The repository's plus_drive/ never reaches these tests."""
    monkeypatch.setenv("DIGI_PLUS_DRIVE", str(tmp_path_factory.mktemp("no-plus-drive")))


@pytest.fixture(autouse=True)
def _isolate_second_core(monkeypatch):
    """One-device tests must not discover local cores or a second engine."""
    monkeypatch.setenv("DIGI_EMU_SECOND_CORE", "0")


def launcher():
    return runpy.run_path(
        str(ROOT / "tools/emulator/native_emu.py"), run_name="launcher_test"
    )


def _set_table(monkeypatch, entries):
    """Make the tier config hold ENTRIES (digest -> entry) in place of config/emulator.json."""
    from tools.emulator import emulator_config

    config = {"_contexts": {"workbench": {"SHARC_FAST_AUTO": "4,2,0"}}, **entries}
    monkeypatch.setattr(digi_env, "tier_config", lambda: config)
    monkeypatch.setattr(
        digi_env, "fast_table", lambda: emulator_config.firmware_table(config)
    )
    return entries


def _known_syx(tmp_path, scope, monkeypatch, content=b"dn2", **extra):
    syx = tmp_path / "fw.syx"
    syx.write_bytes(content)
    digest = __import__("hashlib").sha256(content).hexdigest()
    entry = {"fast_regions": "0x10,0x20", **extra}
    monkeypatch.setattr(
        digi_env, "_syx_digest", lambda path: digest if Path(path) == syx else ""
    )
    _set_table(monkeypatch, {digest: entry})
    return syx


def test_fast_regions_follow_the_firmware_hash(monkeypatch, tmp_path):
    scope = launcher()
    syx = _known_syx(tmp_path, scope, monkeypatch)
    other = tmp_path / "other.syx"
    other.write_bytes(b"other")
    assert scope["fast_regions_for"](syx) == "0x10,0x20"
    assert scope["fast_regions_for"](other) == ""
    assert scope["fast_regions_for"](tmp_path / "missing.syx") == ""


def _build_run_setup(monkeypatch):
    scope = launcher()
    monkeypatch.setenv("DIGI_EMU_PGO", "0")
    calls = []

    def run(command, **_kwargs):
        calls.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr("subprocess.run", run)
    return scope, calls


def _dual_launch(monkeypatch, tmp_path, first):
    scope, calls = _build_run_setup(monkeypatch)
    inputs, cores = {}, {}
    for device in ("dn2", "dt2"):
        directory = tmp_path / device
        directory.mkdir()
        inputs[device] = {
            label: directory / name
            for label, name in (
                ("firmware", "firmware.syx"),
                ("DSP image", "image.bin"),
                ("DSP state", "state.dsp"),
                ("CF snapshot", "snapshot"),
            )
        }
        for path in inputs[device].values():
            path.touch()
        cores[device] = directory / "generated"
        cores[device].mkdir()
    monkeypatch.delenv("DIGI_EMU_SECOND_CORE")
    from tools.devtools import core_store

    # the core store needs git and cargo: the libraries are tested apart
    monkeypatch.setattr(core_store, "library_env", lambda *a, **k: {})
    monkeypatch.setattr(
        digi_env, "firmware", lambda environ=None: inputs[first]["firmware"]
    )
    monkeypatch.setattr(digi_env, "device_for_syx", lambda path: Path(path).parent.name)
    monkeypatch.setattr(
        digi_env, "ready_inputs", lambda *, device: dict(inputs[device])
    )
    monkeypatch.setattr(
        digi_env, "gen_for_image", lambda image: cores[Path(image).parent.name]
    )
    # Track all variables the launcher can set so no test leaks fast settings.
    for suffix in ("", "_2"):
        for name in (
            "SHARC_FAST_REGIONS",
            "SHARC_BLOCKTRANS",
            "SHARC_FAST_FALLBACK_REGIONS",
        ):
            monkeypatch.setenv(name + suffix, "")
            monkeypatch.delenv(name + suffix)
    monkeypatch.delenv("DIGI_EMU_FAST", raising=False)
    monkeypatch.delenv("SHARC_FAST_AUTO", raising=False)
    _set_table(
        monkeypatch,
        {
            device: {
                "fast_regions": f"{device}-regions",
                "blocktrans": f"{device}-roots",
                "fallback_regions": f"{device}-fallback",
            }
            for device in ("dn2", "dt2")
        },
    )
    monkeypatch.setattr(digi_env, "_syx_digest", lambda path: Path(path).parent.name)
    return scope, calls, inputs, cores


@pytest.mark.parametrize("first", ["dn2", "dt2"])
@pytest.mark.parametrize("positional", [False, True])
def test_desktop_defaults_to_both_devices_with_per_core_fast_settings(
    monkeypatch, tmp_path, capsys, first, positional
):
    scope, calls, inputs, cores = _dual_launch(monkeypatch, tmp_path, first)
    other = "dt2" if first == "dn2" else "dn2"
    scope["main"]([str(inputs[first]["firmware"])] if positional else [])
    forwarded = calls[-1][1:]
    if positional:
        assert forwarded[0] == str(inputs[first]["firmware"])
    else:
        assert forwarded[0].startswith("--")
        assert forwarded[forwarded.index("--audio-profile-syx") + 1] == str(
            inputs[first]["firmware"]
        )
    at = forwarded.index("--also-coupled")
    assert forwarded[at + 1 : at + 5] == [
        str(inputs[other][label])
        for label in ("DSP image", "DSP state", "CF snapshot", "firmware")
    ]
    # The launch names no generated core for the host: it holds none.
    assert "SHARC_GEN_DIR" not in os.environ
    # Each core reads the lists with the suffix of its device, whichever firmware launches.
    for suffix, device in (("", "dn2"), ("_2", "dt2")):
        for name, value in (
            ("SHARC_FAST_REGIONS", "regions"),
            ("SHARC_BLOCKTRANS", "roots"),
            ("SHARC_FAST_FALLBACK_REGIONS", "fallback"),
        ):
            assert os.environ[name + suffix] == f"{device}-{value}"


@pytest.mark.parametrize("first", ["dn2", "dt2"])
@pytest.mark.parametrize("also", [True, False])
def test_a_coupled_launch_gets_the_libraries_of_both_devices(
    monkeypatch, tmp_path, first, also
):
    from tools.devtools import core_store

    scope, _calls, inputs, cores = _dual_launch(monkeypatch, tmp_path, first)
    other = "dt2" if first == "dn2" else "dn2"
    asked = {}
    monkeypatch.setattr(
        core_store,
        "library_env",
        lambda devices, *a, **k: (
            asked.update(devices)
            or {f"DIGI_SHARC_CORE_LIB_{next(iter(devices)).upper()}": "y"}
        ),
    )
    args = [
        str(inputs[first]["firmware"]),
        "--auto-coupled",
        "--dsp-image",
        str(inputs[first]["DSP image"]),
    ]
    if also:
        args += [
            "--also-coupled",
            *(
                str(inputs[other][label])
                for label in ("DSP image", "DSP state", "CF snapshot", "firmware")
            ),
        ]
    env = scope["core_library_env"](args)
    assert sorted(env) == ["DIGI_SHARC_CORE_LIB_DN2", "DIGI_SHARC_CORE_LIB_DT2"]
    # the core of each device, from its own registered image
    assert asked == {first: cores[first], other: cores[other]}
    assert scope["core_library_env"](["--cf-only"]) == {}


def test_empty_workbench_uses_individually_trained_libraries(monkeypatch, tmp_path):
    from tools.devtools import core_store
    from tools.emulator import native_pgo

    scope, _calls, _inputs, cores = _dual_launch(monkeypatch, tmp_path, "dn2")
    args = scope["default_audio_args"](
        [], second_core_by_default=True, load_default=False
    )
    profile = tmp_path / "dn2.profdata"
    override = tmp_path / "alternate-dn2"
    monkeypatch.setenv("SHARC_GEN_DIR_DN2", str(override))

    def fresh_profile_candidates(devices, environ):
        assert native_pgo.device_generated("dn2", environ) == override
        assert native_pgo.device_generated("dt2", environ) == cores["dt2"]
        return {"dn2": [profile]}

    monkeypatch.setattr(
        native_pgo, "fresh_profile_candidates", fresh_profile_candidates
    )
    monkeypatch.setattr(native_pgo, "supported_host", lambda: True)
    monkeypatch.delenv("DIGI_EMU_PGO", raising=False)

    def library(devices, *, profiles):
        (device,) = devices
        return {
            f"DIGI_SHARC_CORE_LIB_{device.upper()}": str(profiles.get(device, "plain"))
        }

    monkeypatch.setattr(core_store, "library_env", library)
    env = scope["core_library_env"](args, desktop=True)
    assert env["DIGI_SHARC_CORE_LIB_DN2"] == str(profile)
    assert env["DIGI_SHARC_CORE_LIB_DT2"] == "plain"
    assert json.loads(env["DIGI_DSP_PGO"]) == {"dn2": "ok", "dt2": "plain"}


@pytest.mark.parametrize("single", ["usable", "fails", "missing"])
def test_desktop_retries_a_failed_combined_profile_for_only_that_device(
    monkeypatch, tmp_path, single
):
    from tools.devtools import core_store
    from tools.emulator import native_pgo

    scope, _calls, inputs, _cores = _dual_launch(monkeypatch, tmp_path, "dn2")
    combined = tmp_path / "both.profdata"
    individual = tmp_path / "dn2.profdata"
    monkeypatch.delenv("DIGI_EMU_PGO")
    monkeypatch.setattr(native_pgo, "supported_host", lambda: True)

    def launch_plan(environ, target):
        if target == "both":
            profile = combined
        elif target == "dn2" and single != "missing":
            profile = individual
        else:
            return None, ""
        return native_pgo.LaunchPlan([], [], {}, str(profile), target), ""

    monkeypatch.setattr(native_pgo, "launch_plan", launch_plan)
    attempts = {"dn2": [], "dt2": []}

    def library(devices, *, profiles=None):
        (device,) = devices
        profile = (profiles or {}).get(device)
        attempts[device].append(profile)
        if device == "dn2" and (
            profile == combined or (profile == individual and single == "fails")
        ):
            raise core_store.CoreStoreError("profile build failed")
        return {f"DIGI_SHARC_CORE_LIB_{device.upper()}": str(profile or "plain")}

    monkeypatch.setattr(core_store, "library_env", library)
    args = [str(inputs["dn2"]["firmware"]), "--auto-coupled"]
    env = scope["core_library_env"](args, desktop=True)
    expected = {
        "usable": [combined, individual],
        "fails": [combined, individual, None],
        "missing": [combined, None],
    }[single]
    assert attempts["dn2"] == expected
    assert attempts["dt2"] == [combined]
    assert env["DIGI_SHARC_CORE_LIB_DN2"] == (
        str(individual) if single == "usable" else "plain"
    )
    assert env["DIGI_SHARC_CORE_LIB_DT2"] == str(combined)
    assert json.loads(env["DIGI_DSP_PGO"]) == {
        "dn2": "ok" if single == "usable" else "plain",
        "dt2": "ok",
    }


def test_a_missing_library_of_one_device_does_not_stop_the_launch(
    monkeypatch, tmp_path, capsys
):
    from tools.devtools import core_store

    scope, _calls, inputs, _cores = _dual_launch(monkeypatch, tmp_path, "dn2")

    def library_env(devices, *a, **k):
        (device,) = devices
        if device == "dt2":
            raise core_store.CoreStoreError("no gen")
        return {"DIGI_SHARC_CORE_LIB_DN2": "y"}

    monkeypatch.setattr(core_store, "library_env", library_env)
    args = [str(inputs["dn2"]["firmware"]), "--auto-coupled"]
    assert scope["core_library_env"](args) == {"DIGI_SHARC_CORE_LIB_DN2": "y"}
    err = capsys.readouterr().err
    assert (
        "DT2 core library unavailable" in err
        and "verify_build cores --device dt2" in err
    )
    monkeypatch.setattr(
        core_store,
        "library_env",
        lambda *a, **k: (_ for _ in ()).throw(core_store.CoreStoreError("no gen")),
    )
    with pytest.raises(SystemExit):
        scope["core_library_env"](args)


# plus_drive/<device>/ is the +Drive input.


def _folder(root, device, files):
    for name, data in files.items():
        path = root / device / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    return root / device


def test_card_cache_key_follows_every_file(monkeypatch, tmp_path):
    scope = launcher()
    glob = scope["plusdrive_card"].__globals__
    target = tmp_path / "target"
    (target / "release").mkdir(parents=True)
    (target / "release" / "plusdrive").write_bytes(b"binary")
    monkeypatch.setitem(glob, "target_dir_for", lambda gen=None: str(target))
    monkeypatch.setitem(glob, "ROOT", tmp_path / "repo")
    syx = tmp_path / "fw.syx"
    syx.write_bytes(b"fw")
    folder = _folder(
        tmp_path, "dt2", {"a/b.bin": b"1", "c.dkm": b"2", ".mmcfs/kits/0000.bin": b"3"}
    )
    built = []

    def run(command, **_kwargs):
        if command[0].endswith("plusdrive"):
            built.append(command)
            Path(command[command.index("-o") + 1]).write_bytes(b"img")
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr("subprocess.run", run)
    card = scope["plusdrive_card"]
    first = card(folder, syx, {}, "dt2")
    assert card(folder, syx, {}, "dt2") == first and len(built) == 1
    assert first.name == "dt2.img"
    assert "--device" in built[0] and built[0][built[0].index("--device") + 1] == "dt2"
    (folder / "a" / "b.bin").write_bytes(b"changed")
    second = card(folder, syx, {}, "dt2")
    assert second != first and len(built) == 2
    (folder / "c.dkm").rename(folder / "d.dkm")
    assert card(folder, syx, {}, "dt2") not in (first, second)
    assert card(folder, syx, {}, "dn2").name == "dn2.img"
    with pytest.raises(SystemExit):
        card(tmp_path / "missing", syx, {}, "dt2")


# --- a device without DSP, runtime or card ---------------------------------------------------------------------------------


def _no_dsp_device(monkeypatch, tmp_path):
    """The short id of an identity-only device file (no DSP, no `[digikit]`, no card), in a devices folder of its own."""
    folder = tmp_path / "devices"
    folder.mkdir()
    (folder / "identity-only.toml").write_text(
        '[device]\nname = "Identity Only"\nshort = "idonly"\n\n[capabilities]\ndsp = "none"\ncard = "none"\n'
    )
    monkeypatch.setenv("DT2_DEVICES", str(folder))
    device = device_data.by_short("idonly")
    assert not device.has_dsp() and not device.runtime_target and not device.has_card()
    return device.short


def _launch_setup(monkeypatch, tmp_path, device):
    """main() with the device lookup stubbed and no card ever built; returns the launcher, the commands it ran and a firmware file."""
    scope, calls = _build_run_setup(monkeypatch)
    syx = tmp_path / "known.syx"
    syx.write_bytes(b"x")
    monkeypatch.setattr(digi_env, "device_for_syx", lambda path: device)
    monkeypatch.setattr(digi_env, "patch_base", lambda path: None)
    monkeypatch.setitem(
        scope["main"].__globals__,
        "plusdrive_card",
        lambda *a, **k: pytest.fail("no card is built for this device"),
    )
    return scope, calls, syx


def test_a_device_without_a_card_refuses_a_card_folder(monkeypatch, tmp_path):
    scope, calls, syx = _launch_setup(
        monkeypatch, tmp_path, _no_dsp_device(monkeypatch, tmp_path)
    )
    with pytest.raises(SystemExit, match="no card"):
        scope["main"]([str(syx), "--plus-drive", str(tmp_path)])


def test_a_device_that_is_not_a_runtime_target_refuses_runtime(monkeypatch, tmp_path):
    scope, calls, syx = _launch_setup(
        monkeypatch, tmp_path, _no_dsp_device(monkeypatch, tmp_path)
    )
    with pytest.raises(SystemExit, match="not a digikit runtime target"):
        scope["main"]([str(syx), "--runtime"])


def test_an_unknown_firmware_is_an_error_not_a_default_device(monkeypatch, tmp_path):
    scope, calls, syx = _launch_setup(monkeypatch, tmp_path, None)
    with pytest.raises(SystemExit, match="cannot tell which device"):
        scope["main"]([str(syx)])
    assert calls == []


def test_a_patched_build_keeps_the_audio_state_of_the_firmware_the_snapshot_was_taken_from(
    tmp_path,
):
    """The ready snapshot is valid for one firmware: a patched build (another hash) must not become the audio profile, or the daemon refuses the snapshot."""
    import hashlib
    import json

    scope = launcher()
    stock = tmp_path / "stock.syx"
    stock.write_bytes(b"stock")
    patched = tmp_path / "patched.syx"
    patched.write_bytes(b"patched")
    snapshot = tmp_path / "digi-audio-m5.snap"
    paths = {"CF snapshot": snapshot}
    check = scope["ready_is_of_another_firmware"]
    assert check(paths, patched) is False, (
        "a directory that records nothing is not known to differ"
    )
    (tmp_path / "SOURCE.json").write_text(
        json.dumps({"syx_sha256": hashlib.sha256(b"stock").hexdigest()})
    )
    assert check(paths, stock) is False
    assert check(paths, patched) is True
