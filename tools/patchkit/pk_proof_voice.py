#!/usr/bin/env python3
"""patchkit core/dsp-voice on the Digitakt II 1.17: with no machine loaded the per-voice hook changes nothing.

    uv run python -m tools.patchkit.pk_proof_voice [--patched OUT.syx] [--ready DIR] [--out DIR] [--quick] [--jobs N]

Each case starts a coupled daemon on the stock firmware and the one-sample card, optionally loads the patched image
(`firmware.load`, hot DSP) and renders TRIG 1 held with PCM capture:

  stock           the stock firmware (reference)
  patched-type0   core/dsp-voice alone, no machine module: the PCM must be bit-identical to stock (the hook with no
                  registered type renders a stock machine untouched)

The patched image is built from core/dsp-voice unless `--patched` names one. Output stays under out/ (derived from firmware).
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys

import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402

PATCH_MODS = ["core/dsp-voice"]
TRIG1 = 25


def analyse(pcm: np.ndarray) -> dict:
    left = pcm[0::2].astype(np.float64)
    act = np.flatnonzero(np.abs(left) > 1e-6)
    return {"active_samples": int(len(act)), "silent": len(act) < 4000}


class VoiceProof(K.Proof):
    tool = "voice"
    device = "dt2"
    fw = "1.17"
    patched: pathlib.Path | None = (
        None  # --patched: an image to load instead of building PATCH_MODS
    )

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.report: dict = {}
        self.pcm: dict[str, np.ndarray] = {}

    def render(self, name: str) -> dict:
        """One case from `restart` on its own daemon."""
        if name in self.report:
            return self.report[name]
        use_patch = name == "patched-type0"
        wav = self.out / f"{name}.wav"
        with self.session(
            PATCH_MODS if use_patch else [],
            f"case-{'p' if use_patch else 's'}",
            image=self.patched if use_patch else None,
        ) as s:
            emu = s.emu
            emu.restart(auto_no=True)
            emu.step(20_000_000)
            emu.call("pcm.tap", action="capture", path=str(wav))
            emu.call("input.button", code=TRIG1, down=True)
            emu.step(40_000_000)
            frames = emu.status()["dsp_frames"]
            emu.call("run_until", frames=frames + 1500)
            emu.call("input.button", code=TRIG1, down=False)
            frames = emu.status()["dsp_frames"]
            emu.call("run_until", frames=frames + 300)
            s.timings["emu_ms"] = s.timings.get("emu_ms", 0.0) + (
                60_000_000 / 132_000 + 1800 / 1.5
            )
            emu.call("pcm.tap", action="capture_stop")
            st = emu.status()
        info = {
            "dsp_halted": (st.get("audio") or {}).get("halted"),
            "frames": st["dsp_frames"],
        }
        self.pcm[name] = hook_ab.read_wav(wav)
        info.update(analyse(self.pcm[name]))
        self.report[name] = info
        print(name, json.dumps(info), flush=True)
        return info

    def part_type0(self) -> None:
        stock, patched = self.render("stock"), self.render("patched-type0")
        n = min(len(self.pcm["stock"]), len(self.pcm["patched-type0"]))
        same = bool(
            np.array_equal(self.pcm["stock"][:n], self.pcm["patched-type0"][:n])
        )
        self.check(
            "core/dsp-voice with no machine: the PCM equals stock",
            same
            and not patched["silent"]
            and stock["dsp_halted"] is None
            and patched["dsp_halted"] is None,
            f"equal to stock: {same}, {patched['active_samples']} active samples",
        )
        (self.out / "report.json").write_text(
            json.dumps(self.report, indent=1, default=float)
        )


PARTS = [
    K.Part(
        "type0",
        "stock, and core/dsp-voice alone equals it",
        VoiceProof.part_type0,
        True,
        60,
    ),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--patched",
        type=pathlib.Path,
        help="an image to load instead of the patch of PATCH_MODS",
    )
    ap.add_argument(
        "--ready", type=pathlib.Path, help="ready inputs (DIGI_PK_READY_DT2_117)"
    )
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or ROOT / "out" / "runs" / "voice-proof"
    if a.ready:
        os.environ["DIGI_PK_READY_DT2_117"] = str(a.ready.resolve())
    VoiceProof.patched = a.patched.resolve() if a.patched else None
    return K.run_tool(__file__, VoiceProof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
