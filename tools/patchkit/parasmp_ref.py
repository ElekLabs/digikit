"""tom/parasmp's float32 model, operation for operation as modules/machines/parasmp/src/dsp/parasmp.s: the slot's sample as a
one-shot played paraphonically, each trig taking the next of four voices with its own note and amp envelope."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import numpy as np

from tools.patchkit.pluck_ref import exp2, f32

_GEN = (
    Path(__file__).resolve().parents[2]
    / "modules/machines/parasmp/src/dsp/gen_tables.py"
)
_spec = importlib.util.spec_from_file_location("parasmp_gen_tables", _GEN)
assert _spec and _spec.loader
gen = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gen)

K = {name: f32(v) for name, v in gen.consts()}
PAN = [f32(v) for v in gen.pan()]
ATK = [f32(v) for v in gen.ainc()]
DEC = [f32(v) for v in gen.dcoef()]
PAN_N = gen.PAN_N
OFF, ATTACK, HELD, DECAY = 0, 1, 2, 3


def loop(word: int, n: int) -> tuple[int, int]:
    """LOOP (bipolar, 0x4000 OFF) as (mode, loop start in frames): 0 none, 1 forward, -1 ping-pong."""
    d = word - 0x4000
    if -0x100 < d < 0x100:
        return 0, -1
    f = min(f32(f32(abs(d) - 0x100) * K["LP"]), K["ONE"])
    return (1 if d > 0 else -1), min(int(f32(f * f32(n))), n - 2)


def read(ch: np.ndarray, k: int) -> np.float32:
    return f32(np.int32(ch[k]) << 16)


class ParaSmp:
    """Params: the six knob words (LOOP, DTUN, ATK, DEC, STRT, SPRD). `note_on(pitch)` at each trig, `block(params, released)` per
    32 samples: (left, right, done)."""

    def __init__(self, left: np.ndarray, right: np.ndarray | None = None) -> None:
        self.left = np.asarray(left, np.int16)
        self.right = self.left if right is None else np.asarray(right, np.int16)
        self.pos = [f32(0.0)] * 4
        self.back = [False] * 4
        self.lvl = [f32(0.0)] * 4
        self.stg = [OFF] * 4
        self.pit = [0] * 4
        self.new = [False] * 4
        self.next = 0

    def note_on(self, pitch: int) -> int:
        k = self.next
        self.next = (k + 1) & 3
        self.pit[k] = pitch
        self.stg[k] = ATTACK
        self.new[k] = True
        return k

    def note_on_notes(self, pitch: int, notes: int = 0) -> list[int]:
        """Note 1 and core/polynotes' notes 2-4 (NOTES: a byte each from the low one, 0x40 + the interval from note 1; 0, 0x40 and
        0x80 up are none), each in the next slot."""
        slots = [self.note_on(pitch)]
        for i in range(3):
            b = (notes >> (8 * i)) & 0xFF
            if 0 < b < 0x80 and b != 0x40:
                slots.append(self.note_on(pitch + 256 * (b - 0x40)))
        return slots

    def voice(
        self, k: int, params: list[int]
    ) -> tuple[np.float32, np.float32, np.float32]:
        d = f32(f32(params[1] - 0x4000) * K["DTN"])
        d = max(min(d, K["ONE"]), K["M1"])
        d = f32(d * f32(d * d))
        dw = f32(d * K["DMAX"])
        s = K["ONE"] if k % 2 == 0 else K["M1"]
        if k >= 2:
            s = f32(s * K["HALF"])
        x = f32(f32(f32(self.pit[k] - gen.NOTE0) + f32(dw * s)) * K["SEMI"])
        x = max(min(x, K["OCTMAX"]), f32(-K["OCTMAX"]))
        qs = int(f32(f32(f32(params[5]) * K["SPR"]) + K["HALF"]))
        side = qs >> 1 if k >= 2 else qs
        q = PAN_N // 2 + (side if k % 2 == 0 else -side)
        return f32(exp2(x)), f32(PAN[q] * K["GAIN"]), f32(PAN[PAN_N - q] * K["GAIN"])

    def envelope(self, k: int, params: list[int], n: int) -> np.ndarray:
        ai, dc = ATK[params[2] >> 8], DEC[params[3] >> 8]
        env = np.zeros(n, np.float32)
        for i in range(n):
            if self.stg[k] == ATTACK:
                self.lvl[k] = f32(self.lvl[k] + ai)
                if self.lvl[k] >= K["ONE"]:
                    self.lvl[k] = K["ONE"]
                    self.stg[k] = HELD
            elif self.stg[k] == DECAY:
                self.lvl[k] = f32(self.lvl[k] * dc)
                if self.lvl[k] < K["OFF"]:
                    self.lvl[k] = f32(0.0)
                    self.stg[k] = OFF
            env[i] = self.lvl[k]
        return env

    def block(
        self, params: list[int], released: bool = False, n: int = 32
    ) -> tuple[np.ndarray, np.ndarray, bool]:
        if released:
            self.stg = [DECAY if s in (ATTACK, HELD) else s for s in self.stg]
        left, right = np.zeros(n, np.float32), np.zeros(n, np.float32)
        frames = len(self.left)
        if frames < 4:
            return left, right, False
        mode, ls = loop(params[0], frames)
        nf, lsf, last = f32(frames), f32(ls), f32(frames - 1)
        start = min(
            int(f32(min(f32(f32(params[4]) * K["W01"]), K["ONE"]) * nf)), frames - 1
        )
        stereo = self.right is not self.left
        for k in range(4):
            if self.stg[k] == OFF:
                continue
            if self.new[k]:
                self.pos[k] = f32(start)
                self.back[k] = False
                self.new[k] = False
            rate, gl, gr = self.voice(k, params)
            env = self.envelope(k, params, n)
            ph = self.pos[k]
            for i in range(n):
                if mode < 0:
                    while ph > last or ph < lsf:
                        if ph > last:
                            ph = f32(f32(last + last) - ph)
                            self.back[k] = True
                        else:
                            ph = f32(f32(lsf + lsf) - ph)
                            self.back[k] = False
                else:
                    while ph >= nf:
                        if mode == 0:
                            self.stg[k] = OFF
                            break
                        ph = f32(ph - f32(nf - lsf))
                    if self.stg[k] == OFF:
                        break
                x = int(ph)
                fr = f32(ph - f32(x))
                x1 = x + 1
                if x1 >= frames:
                    x1 = ls if mode > 0 else x
                a, b = read(self.left, x), read(self.left, x1)
                y = f32(a + f32(f32(b - a) * fr))
                y = f32(y * env[i])
                left[i] = f32(left[i] + f32(y * gl))
                if stereo:
                    a, b = read(self.right, x), read(self.right, x1)
                    y = f32(a + f32(f32(b - a) * fr))
                    y = f32(y * env[i])
                right[i] = f32(right[i] + f32(y * gr))
                ph = f32(ph - rate) if self.back[k] else f32(ph + rate)
            self.pos[k] = ph
        return left, right, all(s == OFF for s in self.stg)
