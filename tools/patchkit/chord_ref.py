"""tom/chord's float32 model, operation for operation as modules/machines/chord/src/dsp/chord.s: the slot's sample as a one-shot
played by up to four voices at set intervals, with detune, stereo spread and an optional loop."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import numpy as np

from tools.patchkit.pluck_ref import exp2, f32

_GEN = (
    Path(__file__).resolve().parents[2] / "modules/machines/chord/src/dsp/gen_tables.py"
)
_spec = importlib.util.spec_from_file_location("chord_gen_tables", _GEN)
assert _spec and _spec.loader
gen = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gen)

K = {name: f32(v) for name, v in gen.consts()}
PAN = [f32(v) for v in gen.pan()]
PAN_N = gen.PAN_N
OFF = 0x4000


def interval(word: int) -> int:
    """An INT knob's word in pitch words: 0x4000 is OFF, 0x4100 UNI (0), then +1 semitone per step either way."""
    v = word - 0x4000
    return v - 0x100 if v > 0 else v


def loop(word: int, n: int) -> tuple[int, int]:
    """LOOP (bipolar, 0x4000 OFF) as (mode, loop start in frames): mode 0 none, 1 forward (to the end, then from the start
    again), -1 ping-pong (between the start and the last frame). One step either side of the centre loops from the sample's
    start; the start moves towards the end with the distance."""
    d = word - 0x4000
    if -0x100 < d < 0x100:
        return 0, -1
    f = min(f32(f32(abs(d) - 0x100) * K["LP"]), K["ONE"])
    return (1 if d > 0 else -1), min(int(f32(f * f32(n))), n - 2)


def read(ch: np.ndarray, k: int) -> np.float32:
    """Sample k as the routine reads it: int16 x 2^16."""
    return f32(np.int32(ch[k]) << 16)


class Chord:
    """Params: the six knob words (LOOP, DTUN, INT2, INT3, INT4, SPRD). `left`, `right`: the slot's int16 channels (the same array
    for a mono sample). `note_on()` at a trig, `block(pitch, params)` per 32 samples: (left, right, done)."""

    def __init__(self, left: np.ndarray, right: np.ndarray | None = None) -> None:
        self.left = np.asarray(left, np.int16)
        self.right = self.left if right is None else np.asarray(right, np.int16)
        self.pos = [f32(0.0)] * 4
        self.back = [False] * 4  # a ping-pong voice playing backwards
        self.alive = [False] * 4

    def note_on(self) -> None:
        self.pos = [f32(0.0)] * 4
        self.back = [False] * 4
        self.alive = [True] * 4

    def voices(
        self, pitch: int, params: list[int]
    ) -> list[tuple[int, np.float32, np.float32, np.float32]]:
        """Per sounding voice: (k, rate, left gain, right gain)."""
        _, dtun, *ints, sprd = params
        on = [self.alive[0]] + [
            self.alive[k] and ints[k - 1] != OFF for k in range(1, 4)
        ]
        self.alive = list(
            on
        )  # a voice switched off ends (it comes back at the next trig)
        n = sum(on)
        d = f32(f32(dtun - 0x4000) * K["DTN"])
        d = max(min(d, K["ONE"]), K["M1"])
        d = f32(d * f32(d * d))
        dw = f32(d * K["DMAX"])
        qs = int(f32(f32(f32(sprd) * K["SPR"]) + K["HALF"])) if n > 1 else 0
        g = K[f"G{n}"]
        out, j = [], 0
        for k in range(4):
            if not on[k]:
                continue
            s = K["ONE"] if j % 2 == 0 else K["M1"]
            if j >= 2:
                s = f32(s * K["HALF"])
            iv = interval(ints[k - 1]) if k else 0
            x = f32(f32(f32(pitch + iv - gen.NOTE0) + f32(dw * s)) * K["SEMI"])
            x = max(min(x, K["OCTMAX"]), f32(-K["OCTMAX"]))
            side = qs >> 1 if j >= 2 else qs
            q = PAN_N // 2 + (side if j % 2 == 0 else -side)
            out.append((k, f32(exp2(x)), f32(PAN[q] * g), f32(PAN[PAN_N - q] * g)))
            j += 1
        return out

    def block(
        self, pitch: int, params: list[int], n: int = 32
    ) -> tuple[np.ndarray, np.ndarray, bool]:
        left, right = np.zeros(n, np.float32), np.zeros(n, np.float32)
        frames = len(self.left)
        if frames < 4:
            return left, right, False
        mode, ls = loop(params[0], frames)
        nf = f32(frames)
        lsf = f32(ls)
        last = f32(frames - 1)
        stereo = self.right is not self.left
        for k, rate, gl, gr in self.voices(pitch, params):
            ph = self.pos[k]
            for i in range(n):
                if mode < 0:
                    while ph > last or ph < lsf:  # reflect at the ends
                        if ph > last:
                            ph = f32(f32(last + last) - ph)
                            self.back[k] = True
                        else:
                            ph = f32(f32(lsf + lsf) - ph)
                            self.back[k] = False
                else:
                    while ph >= nf:
                        if mode == 0:
                            self.alive[k] = False
                            break
                        ph = f32(ph - f32(nf - lsf))
                    if not self.alive[k]:
                        break
                x = int(ph)
                fr = f32(ph - f32(x))
                x1 = x + 1
                if x1 >= frames:
                    x1 = ls if mode > 0 else x
                a, b = read(self.left, x), read(self.left, x1)
                y = f32(a + f32(f32(b - a) * fr))
                left[i] = f32(left[i] + f32(y * gl))
                if stereo:
                    a, b = read(self.right, x), read(self.right, x1)
                    y = f32(a + f32(f32(b - a) * fr))
                right[i] = f32(right[i] + f32(y * gr))
                ph = f32(ph - rate) if self.back[k] else f32(ph + rate)
            self.pos[k] = ph
        return left, right, not any(self.alive)
