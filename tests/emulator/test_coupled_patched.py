"""A MAIN-patched firmware cold-boots with DSP audio in the control daemon (spike 0.6, finding 17).

Needs a `digi-emud` built with coupled audio for the device's generated core (DIGI_EMUD_DN2 / DIGI_EMUD_DT2, else
DIGI_EMUD; `tools/emulator/native_emu.py --daemon --build-only <syx>` prints it), the firmware and its ready inputs; run with
`--slow`. DN2 uses firmware/Digitone_II_OS1.12.syx and out/snapshots/dn2-112-ready, DT2 uses firmware/Digitakt_II_OS1.17.syx and
out/snapshots/dt2-117-ready (override with DIGI_PATCHED_DN2_READY / DIGI_PATCHED_DT2_READY and the DIGI_PATCHED_*_SYX
variables). Each test makes a harmless stock-size image (tools/patchkit/patched_image.py).
"""

from __future__ import annotations

import os
import pathlib
import tempfile

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT
from tools.control.digi_client import Emu, EmuError  # noqa: E402
from tools.firmware import config, l2canary  # noqa: E402
from tools.patchkit import patched_audio_check as pac  # noqa: E402

CASES = {
    "dn2": ("Digitone_II_OS1.12.syx", "out/snapshots/dn2-112-ready"),
    "dt2": ("Digitakt_II_OS1.17.syx", "out/snapshots/dt2-117-ready"),
}


def inputs(device: str, monkeypatch: pytest.MonkeyPatch):
    syx_name, ready = CASES[device]
    up = device.upper()
    syx = pathlib.Path(
        os.environ.get(f"DIGI_PATCHED_{up}_SYX") or config.stock(syx_name)
    )
    ready = pathlib.Path(os.environ.get(f"DIGI_PATCHED_{up}_READY") or ROOT / ready)
    emud = os.environ.get(f"DIGI_EMUD_{up}") or os.environ.get("DIGI_EMUD")
    if not (emud and pathlib.Path(emud).is_file() and syx.is_file()):
        pytest.skip(
            "needs a coupled digi-emud (DIGI_EMUD_DN2 / DIGI_EMUD_DT2) and the firmware"
        )
    if not (ready / "digi-audio-m5.snap").is_file():
        pytest.skip(f"needs the ready inputs {ready} (mise run emu:ready)")
    monkeypatch.setenv("DIGI_EMUD", emud)
    return syx, ready


@pytest.mark.slow
@pytest.mark.parametrize("device", ["dn2", "dt2"])
def test_patched_main_boots_with_audio_and_is_cached(device, monkeypatch):
    syx, ready = inputs(device, monkeypatch)
    patched = pac.harmless_patched(syx, syx.stem)
    args = pac.argparse.Namespace(
        device=device, syx=str(syx), ready=str(ready), patched=str(patched),
        run_dir=None, runs=1,
    )  # fmt: skip
    result = pac.run(args)
    cold = result["cold"][0]
    assert cold["start_used"] == "cold" and cold["audio"] is True
    assert cold["boot"]["wall_seconds"] < 30, "uncached coupled boot target"
    assert (
        result["cached"]["start_used"] == "snapshot-cache"
        and result["cached"]["cache_hit"]
    )
    assert result["cached"]["wall_seconds"] < 5
    assert result["restart"]["cache_hit"] is True
    assert result["all_audible"], result
    if device == "dn2":
        assert result["pcm_equal_to_stock"] is True


@pytest.mark.slow
def test_start_snapshot_needs_a_cached_pair_and_start_cold_rebuilds_it(monkeypatch):
    syx, ready = inputs("dn2", monkeypatch)
    patched = pac.harmless_patched(syx, syx.stem)
    with (
        tempfile.TemporaryDirectory(dir="/tmp", prefix="dgp") as run_dir,
        Emu.spawn_coupled(syx, ready, timeout=300, run_dir=run_dir) as emu,
    ):
        with pytest.raises(EmuError, match="no cached pair"):
            emu.call("firmware.load", syx=str(patched), start="snapshot")
        first = emu.call("firmware.load", syx=str(patched), start="auto")
        assert first["start_used"] == "cold" and first["audio"] is True
        emu.load(syx)
        second = emu.call("firmware.load", syx=str(patched), start="snapshot")
        assert second["start_used"] == "snapshot-cache" and second["audio"] is True
        emu.load(syx)
        rebuilt = emu.call("firmware.load", syx=str(patched), start="cold")
        assert rebuilt["start_used"] == "cold" and rebuilt["cache_hit"] is False
        assert emu.status()["cf"]["ready"] is True


@pytest.mark.slow
def test_hot_inject_of_an_appended_stream_block_lands_at_its_address(
    monkeypatch, tmp_path
):
    """A .syx whose DSP stream has an extra block is applied hot (a diff of the loaded memory); the old stream
    cannot take the block back (dropped bytes), and the PCM is unchanged."""
    syx, ready = inputs("dn2", monkeypatch)
    meta = l2canary.build(
        syx, tmp_path / "canary.syx", (0x2008_823C, 0x1000), l2canary.DEFAULT_SEED
    )
    addr, length, seed = 0x2008_823C, 0x1000, l2canary.DEFAULT_SEED
    assert meta["length"] == length
    with Emu.spawn_coupled(syx, ready, timeout=300) as emu:
        assert l2canary.verify(emu, addr, length, seed)["unknown_bytes"] == length
        report = emu.load(tmp_path / "canary.syx")
        assert (
            report["start_used"] == "hot-dsp"
            and report["dsp"]["changed_bytes"] == length
        )
        assert l2canary.verify(emu, addr, length, seed)["intact"]
        back = emu.load(syx)
        assert back["dsp"]["dropped_bytes"] == length


@pytest.mark.slow
def test_a_patched_image_from_the_command_line_soft_resets(monkeypatch, tmp_path):
    """A digikit image given to the daemon as its firmware argument (`mise run emu:serve -- out/patch/X.syx`) is loaded as stock,
    without an analysis and with itself as its base. A guest soft reset (the runtime's restart, RCM.RCR) used to return silently and
    the guest spun for ever; it now finds the base from the patch record, cold-boots the image again and the guest runs on.
    About 3 minutes: the image build, the ready state for its card, the boot and the restart's cold boot."""
    import time

    from tools.device import dkupload
    from tools.emulator import native_emu
    from tools.patchkit import pk_dkrt_boot

    _, _ = inputs("dt2", monkeypatch)
    env = pk_dkrt_boot.Env("dt2-1.17", tmp_path)
    img = env.build_image("core/runtime,core/rows,core/dyndsp")
    args = native_emu.default_audio_args(
        [str(img.syx), "--card-image", str(env.card(None))]
    )
    assert "--auto-coupled" in args, "no coupled ready inputs for this card"
    with Emu.spawn(*args, timeout=600, run_dir=tmp_path / "run") as emu:
        emu.subscribe()
        emu.step(2_000_000)
        before = emu.status()["cf"]["mainloop_hits"]
        emu.drain_events()
        dkupload.Uploader(dkupload.EmuTransport(emu)).restart()
        seen = []
        t0 = time.monotonic()
        while time.monotonic() - t0 < 300:
            try:
                emu.step(400_000)
            except EmuError:  # steps are refused while the cold boot runs on its worker
                time.sleep(0.5)
            seen += emu.drain_events()
            if any(e["kind"] == "firmware.loaded" for e in seen):
                break
            assert not any(e.get("restart") == "unsupported" for e in seen), seen
        resets = [e for e in seen if e["kind"] == "cpu.reset"]
        assert resets and resets[0]["restart"] == "cold boot started", seen
        assert any(e["kind"] == "firmware.loaded" and e["ok"] for e in seen), seen
        emu.step(20_000_000)
        assert emu.status()["cf"]["mainloop_hits"] > 0
        assert before > 0
