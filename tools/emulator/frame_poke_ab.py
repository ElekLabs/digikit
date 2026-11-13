#!/usr/bin/env python3
"""Does the DSP ignore a TX-frame word? A/B of scenario PCM with DSP_FRAME_POKE.

    uv run python -m tools.emulator.frame_poke_ab dt2 --file tests/regression/scenarios/dt2-sendfx.ts \\
        --scenario sf-del-ref --scenario sf-rev-ref --poke 0x6f8=0x7f00 --poke 0x6f8=0

Each scenario runs unpatched and once per --poke (OFF=VAL[/TRACKS], see DSP_FRAME_POKE in
packages/emulator/boot/src/sharc_peer.rs: the big-endian 16-bit word at byte OFF of every frame sent to
the DSP). A poke whose PCM hash equals the unpatched one for every scenario is evidence, for
those scenarios only, that the word is not read (or has no effect). A scenario must exercise
the stage that would read the word (finding 04, frame map).
"""

from __future__ import annotations

import argparse
import os
import pathlib
import sys
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools.emulator import coupled, digi_env  # noqa: E402
from tools.firmware import device as device_files  # noqa: E402


def run(
    device: str,
    file: pathlib.Path,
    names: list[str],
    out: pathlib.Path,
    poke: str | None,
) -> dict[str, str]:
    old = os.environ.pop("DSP_FRAME_POKE", None)
    if poke:
        os.environ["DSP_FRAME_POKE"] = poke
    try:
        rep: dict[str, Any] = coupled.run_batch(
            device, file, names, out, jobs=coupled.default_jobs()
        )
    finally:
        os.environ.pop("DSP_FRAME_POKE", None)
        if old is not None:
            os.environ["DSP_FRAME_POKE"] = old
    return {s["name"]: s["pcm"]["sha256"] for s in rep["scenarios"]}


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("device", choices=device_files.shorts())
    p.add_argument("--file", type=pathlib.Path, required=True)
    p.add_argument("--scenario", action="append", required=True)
    p.add_argument("--poke", action="append", required=True)
    a = p.parse_args(argv)
    if a.device == "dt2":
        coupled.enable_dt2_card()
    base_dir = digi_env.run_dir("frame-poke")
    base = run(a.device, a.file, a.scenario, base_dir / "base", None)
    for i, poke in enumerate(a.poke):
        got = run(a.device, a.file, a.scenario, base_dir / f"poke{i}", poke)
        for n in a.scenario:
            same = got[n] == base[n]
            print(
                f"{poke:24s} {n:20s} {'identical' if same else 'CHANGED  ' + got[n][:12]}"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
