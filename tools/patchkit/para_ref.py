"""tom/para's float32 model, operation for operation as modules/machines/para/src/dsp/para.s."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import numpy as np

from tools.patchkit.pluck_ref import exp2, f32

_GEN = (
    Path(__file__).resolve().parents[2] / "modules/machines/para/src/dsp/gen_tables.py"
)
_spec = importlib.util.spec_from_file_location("para_gen_tables", _GEN)
assert _spec and _spec.loader
gen = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gen)

K = {name: f32(v) for name, v in gen.consts()}
PAN = [f32(v) for v in gen.pan()]
ATK = [f32(v) for v in gen.ainc()]
DEC = [f32(v) for v in gen.dcoef()]
FRAME = gen.FRAME
PAN_N = gen.PAN_N
OFF, ATTACK, HELD, DECAY = 0, 1, 2, 3


def serum_table(frames: int, size: int = FRAME) -> np.ndarray:
    """int16 frames that morph from a sine to a saw-like stack of harmonics: frame f has 1 + f harmonics at 1/h."""
    t = np.arange(size) / size
    out = []
    for f in range(frames):
        w = sum(np.sin(2 * np.pi * h * t) / h for h in range(1, f + 2))
        out.append(np.round(w / np.max(np.abs(w)) * 0.8 * 32767))
    return np.concatenate(out).astype(np.int16)


class Para:
    """Params: the six knob words (WAVE, DTUN, ATK, DEC, JUMP, SPRD). `table`: the sample's int16 samples. `note_on(pitch)` at
    each trig, `block(params, released)` per 32 samples: (left, right, done)."""

    def __init__(self, table: np.ndarray) -> None:
        self.table = np.asarray(table, np.int16)
        self.smooth = True
        self.scaled = self.table.astype(np.float32) * np.float32(
            65536
        )  # as the routine reads them
        self.ph = [f32(0.0)] * 4
        self.lvl = [f32(0.0)] * 4
        self.stg = [OFF] * 4
        self.pit = [0] * 4
        self.next = 0

    def note_on(self, pitch: int) -> int:
        k = self.next
        self.next = (k + 1) & 3
        self.pit[k] = pitch
        self.stg[k] = ATTACK
        self.ph[k] = f32(0.0)
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

    def frames(
        self, params: list[int]
    ) -> tuple[int, int, list[tuple[int, int]], np.float32]:
        """W, the frame count, (frame A, frame B) per voice slot, and the fraction between them."""
        n = len(self.table)
        w = FRAME if n >= FRAME else n
        nf = n // FRAME if n >= FRAME else 1
        p = f32(f32(min(f32(f32(params[0]) * K["W01"]), K["ONE"])) * f32(nf - 1))
        f0 = int(p)
        fr = f32(p - f32(f0))
        if not self.smooth:  # core/trackflags bit 0 (FUNC + press WAVE): the nearer frame alone
            fr = K["ONE"] if fr >= K["HALF"] else f32(0.0)
        jump = (params[4] >> 8) - 64
        out = []
        for k in range(4):
            a = f0 + k * jump
            while a >= nf:
                a -= nf
            while a < 0:
                a += nf
            out.append((a, min(a + 1, nf - 1)))
        return w, nf, out, fr

    def voice(
        self, k: int, params: list[int], w: int
    ) -> tuple[np.float32, np.float32, np.float32]:
        """Voice slot k's increment and its two gains."""
        d = f32(f32(params[1] - 0x4000) * K["DTN"])
        d = max(min(d, K["ONE"]), K["M1"])
        d = f32(d * f32(d * d))
        dw = f32(d * K["DMAX"])
        s = K["ONE"] if k % 2 == 0 else K["M1"]
        if k >= 2:
            s = f32(s * K["HALF"])
        x = f32(f32(f32(self.pit[k]) + f32(dw * s)) * K["SEMI"])
        x = max(min(x, K["OCTMAX"]), f32(0.0))
        inc = f32(f32(f32(exp2(x) * K["NOTE0"]) * K["INVFS"]) * f32(w))
        qs = int(f32(f32(f32(params[5]) * K["SPR"]) + K["HALF"]))
        side = qs >> 1 if k >= 2 else qs
        q = PAN_N // 2 + (side if k % 2 == 0 else -side)
        return inc, f32(PAN[q] * K["GAIN"]), f32(PAN[PAN_N - q] * K["GAIN"])

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
        if len(self.table) < 4:
            return left, right, False
        w, _, fab, fr = self.frames(params)
        for k in range(4):
            if self.stg[k] == OFF:
                continue
            inc, gl, gr = self.voice(k, params, w)
            env = self.envelope(k, params, n)
            a = self.scaled[fab[k][0] * w : fab[k][0] * w + w]
            b = self.scaled[fab[k][1] * w : fab[k][1] * w + w]
            ph = self.ph[k] if self.ph[k] < f32(w) else f32(0.0)
            for i in range(n):
                x = int(ph)
                t = f32(ph - f32(x))
                x1 = x + 1 if x + 1 < w else 0
                ya = f32(f32(a[x]) + f32(f32(f32(a[x1]) - f32(a[x])) * t))
                yb = f32(f32(b[x]) + f32(f32(f32(b[x1]) - f32(b[x])) * t))
                y = f32(ya + f32(f32(yb - ya) * fr))
                y = f32(y * env[i])
                left[i] = f32(left[i] + f32(y * gl))
                right[i] = f32(right[i] + f32(y * gr))
                ph = f32(ph + inc)
                if ph >= f32(w):
                    ph = f32(ph - f32(w))
            self.ph[k] = ph
        return left, right, all(s == OFF for s in self.stg)
