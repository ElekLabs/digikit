"""The launcher restores local DN2 audio by default, with CF-only escape hatches."""

import os
import runpy
import subprocess
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT
from tools.emulator import digi_env  # noqa: E402


@pytest.fixture(autouse=True)
def pinned_rustc(monkeypatch):
    """Never ask rustup for the toolchain from these tests."""
    monkeypatch.setattr(digi_env, "toolchain_rustc", lambda: "/fake/bin/rustc")


@pytest.fixture(autouse=True)
def fresh_gen(monkeypatch):
    """The fake gen dirs here carry no CORE_FINGERPRINT."""
    monkeypatch.setenv("DIGI_ALLOW_STALE_GEN", "1")


@pytest.fixture(autouse=True)
def no_plus_drive_folder(monkeypatch, tmp_path_factory):
    """The repository's plus_drive/ never reaches these tests."""
    monkeypatch.setenv("DIGI_PLUS_DRIVE", str(tmp_path_factory.mktemp("no-plus-drive")))


@pytest.fixture(autouse=True)
def _restore_sharc_gen_dir(monkeypatch):
    """native_emu sets SHARC_GEN_DIR with os.environ.setdefault; record it so
    monkeypatch restores it after each test."""
    monkeypatch.setenv("SHARC_GEN_DIR", "x")
    monkeypatch.delenv("SHARC_GEN_DIR")


@pytest.fixture(autouse=True)
def _isolate_second_core(monkeypatch):
    """Single-core tests must not discover local cores or inherit a dual build."""
    monkeypatch.setenv("DIGI_EMU_SECOND_CORE", "0")
    monkeypatch.setenv("SHARC_GEN_DIR_2", "x")
    monkeypatch.delenv("SHARC_GEN_DIR_2")


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


def dt2_syx(monkeypatch, tmp_path):
    syx = tmp_path / "dt2.syx"
    syx.write_bytes(b"dt2")
    monkeypatch.setattr(
        digi_env, "device_for_syx", lambda p: "dt2" if p == syx else None
    )
    return syx


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
    glob = scope["main"].__globals__
    monkeypatch.setitem(glob, "pgo_trained_core", lambda device: None)
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
    # Slot order is fixed: slot 1 is the DN2 core, slot 2 the DT2 core, whichever firmware launches.
    assert os.environ["SHARC_GEN_DIR"] == str(cores["dn2"])
    assert os.environ["SHARC_GEN_DIR_2"] == str(cores["dt2"])
    for suffix, device in (("", "dn2"), ("_2", "dt2")):
        for name, value in (
            ("SHARC_FAST_REGIONS", "regions"),
            ("SHARC_BLOCKTRANS", "roots"),
            ("SHARC_FAST_FALLBACK_REGIONS", "fallback"),
        ):
            assert os.environ[name + suffix] == f"{device}-{value}"


def _dt2_launch(monkeypatch, tmp_path, manifest_core):
    """A DT2 launch with ready inputs, a registered core and a PGO manifest."""
    from tools.emulator import native_pgo

    syx = dt2_syx(monkeypatch, tmp_path)
    ready = tmp_path / "ready-dt2"
    ready.mkdir()
    for name in [
        "digi-audio-dn2-image.bin",
        "digi-audio-m5.snap.dsp",
        "digi-audio-m5.snap",
    ]:
        (ready / name).touch()
    registered = tmp_path / "registered"
    registered.mkdir()
    monkeypatch.setitem(digi_env.READY_DIRS, "dt2", ready)
    monkeypatch.setattr(digi_env, "gen_for_image", lambda _image: registered)
    monkeypatch.setattr(
        native_pgo,
        "read_manifest",
        lambda device="dn2": {"generated_dir": str(manifest_core)},
    )
    # default_audio_args exports SHARC_GEN_DIR: have monkeypatch undo it.
    monkeypatch.setenv("SHARC_GEN_DIR", "x")
    monkeypatch.delenv("SHARC_GEN_DIR")
    return syx, registered


def test_dt2_default_core_skips_a_stale_trained_core(monkeypatch, tmp_path):
    trained = tmp_path / "trained"
    trained.mkdir()
    syx, registered = _dt2_launch(monkeypatch, tmp_path, trained)
    # Only the registered core carries the current fingerprint.
    (registered / digi_env.CORE_FINGERPRINT_FILE).write_text(
        digi_env.core_fingerprint()
    )
    monkeypatch.delenv("DIGI_ALLOW_STALE_GEN")
    launcher()["default_audio_args"]([str(syx)])
    assert os.environ["SHARC_GEN_DIR"] == str(registered)


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
    monkeypatch.setitem(glob, "target_dir_for", lambda gen: str(target))
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
