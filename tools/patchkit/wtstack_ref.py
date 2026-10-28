"""tom/wtstack's float32 model, operation for operation as modules/machines/wtstack/src/dsp/wtstack.s, and Serum-style test tables."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import numpy as np

from tools.patchkit.pluck_ref import exp2, f32

_GEN = (
    Path(__file__).resolve().parents[2]
    / "modules/machines/wtstack/src/dsp/gen_tables.py"
)
_spec = importlib.util.spec_from_file_location("wtstack_gen_tables", _GEN)
assert _spec and _spec.loader
gen = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gen)

K = {name: f32(v) for name, v in gen.consts()}
PAN = [f32(v) for v in gen.pan()]
FRAME = gen.FRAME
PAN_N = gen.PAN_N


def serum_table(frames: int, size: int = FRAME) -> np.ndarray:
    """int16 frames that morph from a sine to a saw-like stack of harmonics: frame f has 1 + f harmonics at 1/h."""
    t = np.arange(size) / size
    out = []
    for f in range(frames):
        w = sum(np.sin(2 * np.pi * h * t) / h for h in range(1, f + 2))
        out.append(np.round(w / np.max(np.abs(w)) * 0.8 * 32767))
    return np.concatenate(out).astype(np.int16)


def interval(word: int) -> int:
    """An INT knob's word in pitch words (256 per semitone): 0x4000 is OFF, 0x4100 UNI (0), then +1 semitone per step either way."""
    v = word - 0x4000
    return v - 0x100 if v > 0 else v


class WtStack:
    """Params: the six knob words (WAVE, DTUN, INT2, INT3, INT4, SPRD). `table`: the sample's int16 samples."""

    def __init__(self, table: np.ndarray) -> None:
        self.table = np.asarray(table, np.int16)
        self.scaled = self.table.astype(np.float32) * np.float32(
            65536
        )  # as the routine reads them
        self.ph = [f32(0.0)] * 4

    def note_on(self) -> None:
        self.ph = [f32(0.0)] * 4

    def voices(
        self, pitch: int, params: list[int]
    ) -> tuple[int, list[tuple[int, np.float32, np.float32, np.float32]]]:
        """W and, per active voice, (k, inc, left gain, right gain)."""
        frames = len(self.table)
        w = FRAME if frames >= FRAME else frames
        wave, dtun, *ints, sprd = params
        on = [True] + [v != 0x4000 for v in ints]
        n = sum(on)
        d = f32(f32(dtun - 0x4000) * K["DTN"])
        d = max(min(d, K["ONE"]), K["M1"])
        d = f32(d * f32(d * d))
        dw = f32(d * K["DMAX"])
        qs = int(f32(f32(f32(sprd) * K["SPR"]) + K["HALF"])) if n > 1 else 0
        g = K[f"G{n}"]
        out = []
        j = 0
        for k in range(4):
            if not on[k]:
                continue
            s = K["ONE"] if j % 2 == 0 else K["M1"]
            if j >= 2:
                s = f32(s * K["HALF"])
            iv = interval(ints[k - 1]) if k else 0
            x = f32(f32(f32(pitch + iv) + f32(dw * s)) * K["SEMI"])
            x = max(min(x, K["OCTMAX"]), f32(0.0))
            inc = f32(f32(f32(exp2(x) * K["NOTE0"]) * K["INVFS"]) * f32(w))
            side = qs >> 1 if j >= 2 else qs
            q = PAN_N // 2 + (side if j % 2 == 0 else -side)
            out.append((k, inc, f32(PAN[q] * g), f32(PAN[PAN_N - q] * g)))
            j += 1
        return w, out

    def block(
        self, pitch: int, params: list[int], n: int = 32
    ) -> tuple[np.ndarray, np.ndarray]:
        frames = len(self.table)
        if frames < 4:
            return np.zeros(n, np.float32), np.zeros(n, np.float32)
        nf = frames // FRAME if frames >= FRAME else 1
        w, vs = self.voices(pitch, params)
        p = f32(f32(min(f32(params[0]) * K["W01"], K["ONE"])) * f32(nf - 1))
        f0 = int(p)
        fr = f32(p - f32(f0))
        f1 = min(f0 + 1, nf - 1)
        a, b = self.scaled[f0 * w : f0 * w + w], self.scaled[f1 * w : f1 * w + w]
        left, right = np.zeros(n, np.float32), np.zeros(n, np.float32)
        for k, inc, gl, gr in vs:
            ph = self.ph[k] if self.ph[k] < f32(w) else f32(0.0)
            for i in range(n):
                x = int(ph)
                t = f32(ph - f32(x))
                x1 = x + 1 if x + 1 < w else 0
                ya = f32(f32(a[x]) + f32(f32(f32(a[x1]) - f32(a[x])) * t))
                yb = f32(f32(b[x]) + f32(f32(f32(b[x1]) - f32(b[x])) * t))
                y = f32(ya + f32(f32(yb - ya) * fr))
                left[i] = f32(left[i] + f32(y * gl))
                right[i] = f32(right[i] + f32(y * gr))
                ph = f32(ph + inc)
                if ph >= f32(w):
                    ph = f32(ph - f32(w))
            self.ph[k] = ph
        return left, right
