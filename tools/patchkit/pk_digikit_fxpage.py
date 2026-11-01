#!/usr/bin/env python3
"""Offline FUNC + FX page profile and PCM oracles, with shared legacy UI helpers.

Device acceptance lives in tests/regression/runtime/fx-pages.test.ts. The fxpersist
and fxshift scenarios still consume the profile, kit, screenshot and knob helpers.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import struct
import tomllib
from pathlib import Path
from typing import Any

import numpy as np

from tools.control.digi_client import oled_png
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.pk_digikit_fx import scaled

ROOT = Path(__file__).resolve().parents[2]

SINE = "test/sine"
FX = "test/dyn-fx-gain"
# DKRT_FXP (modules/core/runtime/src/rs/src/fxpage.rs `FxpState`, repr(C)): byte offsets of its fields
FXP_KIT, FXP_SENT, FXP_ERRORS, FXP_SEEDS = 4, 16, 20, 24
FXP_OWNER, FXP_NKNOB, FXP_CUR, FXP_DSP = 28, 32, 44, 172
W_MIX = 13
ONE = 0x3F800000
# the firmware's FX view list (DEL, REV, CHO): rodata at `cf.page.data.fx_view_list`, the page numbers differ by device
STOCK_PAGES = {"dt2": [13, 14, 15], "dn2": [17, 18, 19]}
# the encoders: knob A and the level knob
ENC_A, ENC_LEVEL = "ENC A", "LEVEL"


def f32(x: float) -> np.float32:
    return np.float32(x)


def bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def unbits(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


class Shots:
    """Screenshots of the OLED: the PNG in the work folder (and in DIGIKIT_SHOTS), and the hash of the raw frame."""

    def __init__(self, work: Path, prefix: str):
        self.dir = work / "shots"
        self.dir.mkdir(parents=True, exist_ok=True)
        self.prefix = prefix
        self.hashes: dict[str, str] = {}

    def take(self, rig: Any, name: str) -> str:
        _, raw = rig.emu.frame()
        png = oled_png(raw, 3)
        fn = f"{self.prefix}-{name}.png"
        (self.dir / fn).write_bytes(png)
        if out := os.environ.get("DIGIKIT_SHOTS"):
            Path(out).mkdir(parents=True, exist_ok=True)
            shutil.copy(self.dir / fn, Path(out) / fn)
        h = hashlib.sha256(raw).hexdigest()
        self.hashes[name] = h
        return h


def open_send_fx(rig: Any) -> None:
    """FUNC + FX: the SEND FX pages, the first (Delay)."""
    rig.s.do_tap(["FUNC", "FX"], 20)
    rig.s.run_ms(400)


def page_down(rig: Any) -> None:
    rig.s.do_tap("DOWN", 20)
    rig.s.run_ms(300)


def fxp(rig: Any) -> int:
    return M.sym(rig.plan, "core/runtime", "DKRT_FXP")


def fxp_words(rig: Any, off: int, n: int) -> list[int]:
    base = fxp(rig) + off
    return [
        struct.unpack(">i", struct.pack(">I", rig.w(base + 4 * i)))[0] for i in range(n)
    ]


def fxp_state(rig: Any) -> dict[str, Any]:
    owner = struct.pack(">I", rig.w(fxp(rig) + FXP_OWNER))
    nknob = struct.pack(">I", rig.w(fxp(rig) + FXP_NKNOB))
    return {
        "kit": rig.w(fxp(rig) + FXP_KIT),
        "sent": rig.w(fxp(rig) + FXP_SENT),
        "errors": rig.w(fxp(rig) + FXP_ERRORS),
        "seeds": rig.w(fxp(rig) + FXP_SEEDS),
        "owner": list(owner),
        "nknob": list(nknob),
        "cur0": fxp_words(rig, FXP_CUR, 8),
        "dsp0": fxp_words(rig, FXP_DSP, 8),
    }


def profile(dev: str) -> dict[str, Any]:
    """`[fx]` of the device file: the facts the runtime and the patch take from it."""
    f = {"dt2": "digitakt-ii", "dn2": "digitone-ii"}[dev]
    return tomllib.loads((ROOT / "devices" / f"{f}.toml").read_text())["fx"]


def u16(rig: Any, addr: int) -> int:
    """The big-endian halfword at ADDR, at any alignment (the Digitone II's kit objects start at odd addresses)."""
    return (
        (rig.w(addr & ~3) >> (8 * (2 - addr % 4)) & 0xFFFF)
        if addr % 4 <= 2
        else ((rig.w(addr & ~3) & 0xFF) << 8 | rig.w((addr & ~3) + 4) >> 24)
    )


def put16(rig: Any, addr: int, raw: int) -> None:
    """Write the big-endian halfword at ADDR, at any alignment (aligned words are read and written whole)."""
    for i, byte in enumerate((raw >> 8 & 0xFF, raw & 0xFF)):
        a = addr + i
        shift = 8 * (3 - a % 4)
        rig.emu.poke32(a & ~3, (rig.w(a & ~3) & ~(0xFF << shift)) | byte << shift)


class Kit:
    """The live kit's array of effect values, as the firmware's pages edit it."""

    def __init__(self, rig: Any, fx: dict[str, Any]):
        self.rig, self.fx = rig, fx

    def raw(self, k: int, j: int) -> int:
        kit = self.rig.w(self.fx["kit_ptr"])
        return u16(
            self.rig,
            kit + self.fx["array_off"] + 2 * self.fx["value_indices"][8 * k + j],
        )

    def set_raw(self, enc: str, k: int, j: int, target: int, tol: int = 0x200) -> int:
        """Turn ENC (bursts of fast events, the sign by the error) until the value reads within TOL of TARGET; returns it. Settles the knob's writes after."""
        for _ in range(120):
            err = target - self.raw(k, j)
            if abs(err) <= tol:
                self.rig.s.run_ms(60)
                return self.raw(k, j)
            self.rig.s.turn_burst(
                enc, 8 if err > 0 else -8, 3 if abs(err) < 0x1000 else 20
            )
        raise RuntimeError(
            f"turning {enc}: value {self.raw(k, j):#x}, wanted {target:#x}"
        )

    def poke(self, index: int, raw: int) -> None:
        """Write the halfword INDEX of the live kit's array (what a page's knob does, without the page)."""
        kit = self.rig.w(self.fx["kit_ptr"])
        put16(self.rig, kit + self.fx["array_off"] + 2 * index, raw)

    def snapshot(self) -> list[int]:
        """All values of the live kit's array (`[fx] tail_off` bytes: 71 on the Digitakt II, 101 on the Digitone II)."""
        kit = self.rig.w(self.fx["kit_ptr"])
        return [
            u16(self.rig, kit + self.fx["array_off"] + 2 * i)
            for i in range(self.fx["tail_off"] // 2)
        ]

    def to_min(self, enc: str) -> None:
        self.rig.s.turn_burst(enc, -8, 80)
        self.rig.s.run_ms(60)

    def to_max(self, enc: str, k: int, j: int) -> None:
        """Turn ENC up in bursts until the value sits at the top (0x7f00). How many events of a burst the firmware takes depends on the
        timing of its UI passes (the encoder accelerates by the time between events), so one burst may stop short."""
        for _ in range(6):
            self.rig.s.turn_burst(enc, 8, 80)
            self.rig.s.run_ms(60)
            if self.raw(k, j) == 0x7F00:
                return


def knob_float(lo: float, hi: float, raw: int) -> float:
    """The runtime's conversion in float32: min + span * (raw / 0x7f00)."""
    return float(f32(lo) + f32(f32(hi) - f32(lo)) * (f32(raw) / f32(32512)))


def main(argv: list[str] | None = None) -> int:
    """Expose the existing PCM oracle without imposing the generic CLI's 2-ULP gate."""
    import argparse
    import json

    from tools.patchkit.hook_ab import read_wav

    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    compare = sub.add_parser("compare")
    compare.add_argument("base", type=Path)
    compare.add_argument("sample", type=Path)
    compare.add_argument("--gain", type=float, required=True)
    args = parser.parse_args(argv)
    print(
        json.dumps(
            scaled(read_wav(args.base), read_wav(args.sample), args.gain),
            allow_nan=False,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
