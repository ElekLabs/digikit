#!/usr/bin/env python3
"""A MAIN-patched firmware must come up in the emulator with DSP audio: load a patched .syx into a coupled daemon,
time the coupled cold boot (uncached, then cached), and play a note.

    uv run python -m tools.patchkit.patched_audio_check dn2|dt2 [--syx BASE.syx] [--patched PATCHED.syx]
        [--ready DIR] [--run-dir DIR] [--runs N]

BASE is the stock firmware (default: the device's default .syx, DIGI_EMU_SYX / DIGI_EMU_SYX_DT2 honoured); READY the
directory of its ready inputs (default: `digi_env.ready_inputs`). Without --patched a harmless one is made with
`tools/patchkit/patched_image.py` (the default project name UNTITLED becomes PATCHKIT) under out/patch/. The daemon is the
`digi-emud` of DIGI_EMUD, built with `mise run emu:serve -- --build-only <syx>`.

The check, in order: the stock firmware plays a note (DN2: TRIG 1 then PLAY; DT2: a one-sample card made by
`card.load`, TRIG 1); the patched firmware is loaded with `start cold` (`force`, so the cache is rebuilt) and plays
the same note; the cached load and `restart` hit the cache; the stock firmware is loaded back. The note must be
audible after every load; because the patch changes a string, not the sound, its PCM equals the stock PCM. The
JSON on stdout has the timings (seconds of wall time) and what the controller reported.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import time
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools import python_env  # noqa: E402
from tools.control.digi_client import Emu  # noqa: E402
from tools.emulator import card_check, digi_env  # noqa: E402
from tools.firmware import device as device_files  # noqa: E402

# DN2: TRIG 1 tapped, then PLAY (the pinned trig1 scenarios' prelude).
DN2_NOTE = [
    {"at_instr": 0, "button": 12, "down": True},
    {"at_instr": 1_000_000, "button": 12, "down": False},
    {"at_instr": 60_000_000, "button": 25, "down": True},
    {"at_instr": 100_000_000, "button": 25, "down": False},
]
TRIG1 = 25


def harmless_patched(syx: pathlib.Path, name: str) -> pathlib.Path:
    """The stock-size test image of tools/patchkit/patched_image.py (no emulator boot, no bootstrap run)."""
    out = ROOT / "out" / "patch" / f"{name}-harmless.syx"
    if not out.is_file() or out.stat().st_mtime < syx.stat().st_mtime:
        subprocess.run(
            [
                sys.executable,
                "-m",
                "tools.patchkit.patched_image",
                "--syx",
                str(syx),
                "--out",
                str(out),
                "--no-bootstrap",
                "--no-boot",
            ],
            check=True,
            capture_output=True,
            cwd=ROOT,
            env=python_env(),
        )
    return out


class Player:
    """Play one note on a coupled daemon and return what the PCM tap saw."""

    def __init__(self, emu: Emu, device: str, work: pathlib.Path):
        self.emu, self.device, self.work = emu, device, work

    def play(self) -> dict[str, Any]:
        emu = self.emu
        if self.device == "dn2":
            emu.restart()
            emu.pcm_start()
            emu.schedule(DN2_NOTE)
            emu.run_until(audio_s=3, timeout_wall_s=600)
            report = emu.pcm_report()
            return {
                "rms": report["rms"],
                "peak": report["peak"],
                "sha256": report["sha256"][:16],
                "audible": report["rms"] > 1e-3,
            }
        wav = self.work / "note.wav"
        emu.step(20_000_000)
        emu.call("pcm.tap", action="capture", path=str(wav))
        emu.call("input.button", code=TRIG1, down=True)
        emu.step(40_000_000)
        emu.call("input.button", code=TRIG1, down=False)
        frames = emu.call("status")["dsp_frames"]
        emu.call("run_until", frames=frames + int(1.5 * 1500))
        emu.call("pcm.tap", action="capture_stop")
        got = card_check.analyse(card_check.read_left(wav))
        got["audible"] = not got.get("silent") and got.get("rms", 0.0) > 1e-3
        got["sha256"] = None
        return got


def run(args: argparse.Namespace) -> dict[str, Any]:
    device = args.device
    syx = pathlib.Path(args.syx or digi_env.firmware_for(device)).resolve()
    ready = pathlib.Path(
        args.ready or digi_env.ready_inputs(device=device)["DSP image"].parent
    )
    patched = (
        pathlib.Path(args.patched).resolve()
        if args.patched
        else harmless_patched(syx, syx.stem)
    )
    work = ROOT / "out" / "patched-audio-check" / device
    work.mkdir(parents=True, exist_ok=True)
    result: dict[str, Any] = {
        "device": device,
        "syx": syx.name,
        "patched": patched.name,
    }
    with Emu.spawn_coupled(syx, ready, timeout=300, run_dir=args.run_dir) as emu:
        if emu.status()["audio"] is None:
            raise SystemExit("this digi-emud was built without coupled audio")
        card = None
        player = Player(emu, device, work)
        if device == "dt2":
            samples = work / "samples"
            card_check.make_samples(samples)
            loaded = emu.call("card.load", dir=str(samples))
            card = loaded["card"]
            result["card_load"] = {
                k: loaded[k] for k in ("seconds", "boot_seconds", "ready_cached")
            }
        result["stock"] = player.play()

        def load(start: str) -> dict[str, Any]:
            params: dict[str, Any] = {
                "syx": str(patched), "start": start, "force": True, "auto_no": True,
            }  # fmt: skip
            if card:
                params["card"] = card
            started = time.monotonic()
            report = emu.call("firmware.load", **params)
            return {
                "wall_seconds": round(time.monotonic() - started, 2),
                "start_used": report["start_used"],
                "cache_hit": report["cache_hit"],
                "audio": report["audio"],
                "boot": report.get("boot"),
                "dsp": report.get("dsp"),
            }

        runs = []
        for _ in range(args.runs):
            cold = load("cold")
            cold["note"] = player.play()
            runs.append(cold)
        result["cold"] = runs
        cached = load("auto")
        cached["note"] = player.play()
        result["cached"] = cached
        started = time.monotonic()
        restart = emu.restart(auto_no=True)
        result["restart"] = {
            "wall_seconds": round(time.monotonic() - started, 2),
            "start_used": restart["start_used"],
            "cache_hit": restart["cache_hit"],
        }
        result["after_restart"] = player.play()
        emu.call(
            "firmware.load",
            syx=str(syx),
            auto_no=True,
            **({"card": card} if card else {}),
        )
        result["stock_again"] = player.play()
    notes = [
        result["stock"],
        *[r["note"] for r in runs],
        cached["note"],
        result["after_restart"],
        result["stock_again"],
    ]
    result["all_audible"] = all(n["audible"] for n in notes)
    result["pcm_equal_to_stock"] = (
        all(n["sha256"] == result["stock"]["sha256"] for n in notes)
        if device == "dn2"
        else None
    )
    return result


def main(argv: list[str]) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("device", choices=device_files.shorts())
    p.add_argument("--syx")
    p.add_argument("--patched")
    p.add_argument("--ready")
    p.add_argument("--run-dir")
    p.add_argument("--runs", type=int, default=1, help="uncached boots to time")
    args = p.parse_args(argv)
    result = run(args)
    print(json.dumps(result, indent=1))
    return 0 if result["all_audible"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
