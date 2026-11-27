#!/usr/bin/env python3
"""Float32 reference of the plucked-string machine (src/dsp/pluck.s of this module) and its tuning analysis.

    PYTHONPATH=<repo> uv run python <this file> tuning [--bright U] [--out FILE.csv]
    PYTHONPATH=<repo> uv run python <this file> measure [NOTE ...]

`Pluck` mirrors the DSP routine operation for operation in numpy float32 (one rounding per SHARC+ operation, no
fused multiply-add), so the concrete-core run and this model agree to the last bits except where the routine's
`FLOAT`/`TRUNC` conventions differ. `tuning` computes, for every MIDI note 0 .. 127, the loop's exact phase delay
at the nominal frequency from the coefficients the routine derives (N, a, b in float32; the exact phase delay of
the damping filter and of the allpass in float64) and prints the cents error of the string's fundamental. The
fundamental of a Karplus-Strong string is the frequency at which the loop's total phase delay equals its period;
`measured_cents` finds it numerically for the actual filters.
"""

from __future__ import annotations

import argparse
import math
import sys

import numpy as np

from tools.patchkit.pitch import f0_from_crossings

f32 = np.float32

FS = 48000.0
INC0 = f32(440 * 2 ** (-69 / 12) / 48000)
P0 = f32(1 / float(INC0))
EXP2_C = [
    f32(c)
    for c in (
        0.9999999403953552,
        0.6931530833244324,
        0.24015361070632935,
        0.0558263324201107,
        0.008989322930574417,
        0.001877583796158433,
    )
]
A_C = [
    f32(c)
    for c in (
        -1.3771490614544746e-07,
        -0.49999943375587463,
        0.2500174641609192,
        -0.12502466142177582,
        0.06215573847293854,
        -0.03095044568181038,
        0.017763901501893997,
        -0.009173204191029072,
    )
]
LINE = 4096
MASK = LINE * 4 - 1


def exp2(x: np.float32) -> np.float32:
    """2^x as `_dv_exp2`: x + 64, truncate, degree-5 polynomial, exact scale (x > -64)."""
    x = f32(x + f32(64.0))
    i = int(x)  # TRUNC
    fr = f32(x - f32(i))
    p = EXP2_C[5]
    for c in (EXP2_C[4], EXP2_C[3], EXP2_C[2], EXP2_C[1], EXP2_C[0]):
        p = f32(f32(p * fr) + c)
    return f32(np.ldexp(p, i - 64))


def tau_lp_exact(b: float, w: float) -> float:
    return math.atan2(b * math.sin(w), (1 - b) + b * math.cos(w)) / w


def tau_ap_exact(a: float, w: float) -> float:
    z = complex(math.cos(w), -math.sin(w))
    h = (a + z) / (1 + a * z)
    return -math.atan2(h.imag, h.real) / w


class Params:
    """The per-block coefficients, exactly as the routine derives them."""

    def __init__(
        self,
        pitch: int,
        tune: int,
        decay: int,
        pick: int,
        bright: int,
        color: int,
        vel: int,
    ) -> None:
        one, half = f32(1.0), f32(0.5)
        inv = f32(1 / 32512.0)
        x = f32(f32(f32(pitch) + f32(f32(tune) - f32(16384.0))) * f32(1 / (256 * 12)))
        # x in 0 .. 11.2: P = P0 2^-x stays at 2.5 or more (the routine's clamp; a word beyond the string's range)
        x = f32(max(min(x, f32(11.2)), f32(0.0)))
        self.inc = f32(exp2(x) * INC0)
        self.P = f32(exp2(f32(-x)) * P0)
        self.w2 = f32(f32(self.inc * self.inc) * f32(4 * math.pi**2))
        ub = f32(min(f32(f32(bright) * inv), one))
        self.b = f32(f32(one - ub) * half)
        self.c1 = f32(one - self.b)
        t = f32(f32(self.b * self.c1) * f32(one - f32(self.b + self.b)))
        t = f32(f32(f32(t * self.w2) * f32(1 / 6)))
        self.tau = f32(self.b - t)
        T = f32(self.P - self.tau)
        # the period the line can hold: N = 2 .. 4090 with R in [0.5, 1.5)
        T = f32(min(max(T, f32(2.501)), f32(4091.49)))
        n = int(f32(T - half))
        self.N = min(max(n, 2), 4090)
        r = f32(T - f32(self.N))
        corr = f32(f32(f32(f32(one - f32(r * r)) * r) * self.w2) * f32(1 / 12))
        self.delta = f32(r - corr)
        y = f32(self.delta - one)
        acc = A_C[7]
        for c in reversed(A_C[:7]):
            acc = f32(f32(acc * y) + c)
        self.a = f32(min(max(acc, f32(-0.9)), f32(0.9)))
        ud = f32(min(f32(f32(decay) * inv), one))
        inv_t60 = f32(exp2(f32(ud * f32(-8.64))) * f32(20.0))
        e = f32(f32(f32(inv_t60 * self.P) * f32(3 * math.log2(10) / 48000)))
        self.g = exp2(f32(-e))
        up = f32(min(f32(f32(pick) * inv), one))
        self.p = int(f32(f32(f32(up * half) * f32(self.N)) + half))
        uc = f32(min(f32(f32(color) * inv), one))
        self.c = f32(uc * f32(0.9))
        vel_n = f32(f32(vel) * inv)
        self.amp = f32(f32(f32(f32(self.c + self.c) + one) * half) * vel_n)

    def loop_delay(self) -> float:
        """N + tau_lp + tau_ap at the nominal frequency, with the exact phase delays of the filters."""
        w0 = 2 * math.pi / float(self.P)
        return (
            self.N + tau_lp_exact(float(self.b), w0) + tau_ap_exact(float(self.a), w0)
        )

    def cents_error(self) -> float:
        """Cents of the string's fundamental against the nominal pitch, from the exact loop phase delay."""
        # the fundamental satisfies D(w) = 2 pi / w; solve by secant iteration from the nominal
        b, a, n = float(self.b), float(self.a), self.N

        def d(w: float) -> float:
            return n + tau_lp_exact(b, w) + tau_ap_exact(a, w) - 2 * math.pi / w

        w1, w2 = (
            2 * math.pi / float(self.P) * 0.999,
            2 * math.pi / float(self.P) * 1.001,
        )
        for _ in range(30):
            d1, d2 = d(w1), d(w2)
            if d2 == d1:
                break
            w1, w2 = w2, w2 - d2 * (w2 - w1) / (d2 - d1)
        f_got = w2 * FS / (2 * math.pi)
        f_want = FS / float(self.P)
        return 1200 * math.log2(f_got / f_want)


class Pluck:
    """One string. `block(params, ...)` renders 32 samples; `note_on` fills the line."""

    def __init__(self) -> None:
        self.line = np.zeros(LINE, dtype=np.float32)
        self.w = 0
        self.lp = f32(0)
        self.ap = f32(0)
        self.quiet = 0
        self.rng = 0x2545F491

    def note_on(self, q: Params) -> None:
        c, c1, amp = q.c, f32(f32(1.0) - q.c), q.amp
        n = q.N
        start = LINE - n - 1
        yf = f32(0)
        vals = []
        total = f32(0)
        x = self.rng
        for _ in range(n + 1):
            x ^= (x << 13) & 0xFFFFFFFF
            x ^= x >> 17
            x ^= (x << 5) & 0xFFFFFFFF
            s = x - (1 << 32) if x >= 1 << 31 else x
            noise = f32(f32(s) * f32(2.0**-31))
            yf = f32(f32(noise * c1) + f32(yf * c))
            v = f32(yf * amp)
            total = f32(total + v)
            vals.append(v)
        self.rng = x
        mean = f32(total * q.inc)
        vals = [f32(v - mean) for v in vals]
        line = self.line
        line[:] = 0  # the routine clears the whole line first
        line[start : start + n + 1] = vals
        if q.p >= 1:
            for k in range(LINE - 1, start + q.p - 1, -1):
                line[k] = f32(line[k] - line[k - q.p])
        self.w = 0
        self.lp = f32(0)
        self.ap = f32(0)
        self.quiet = 0

    def block(self, q: Params) -> np.ndarray:
        out = np.zeros(32, dtype=np.float32)
        r = (self.w - q.N) % LINE
        xprev = self.line[(r - 1) % LINE]
        lp1, ap1 = self.lp, self.ap
        peak = f32(0)
        for k in range(32):
            x = self.line[r]
            r = (r + 1) % LINE
            lp = f32(f32(x * q.c1) + f32(xprev * q.b))
            xprev = x
            ap = f32(f32(f32(lp - ap1) * q.a) + lp1)
            lp1, ap1 = lp, ap
            y = f32(ap * q.g)
            self.line[self.w] = y
            self.w = (self.w + 1) % LINE
            out[k] = y
            peak = max(peak, abs(y))
        self.lp, self.ap = lp1, ap1
        self.quiet = self.quiet + 1 if peak < f32(2.0**-16) else 0
        return out


def cmd_tuning(args: argparse.Namespace) -> int:
    rows = []
    for note in range(128):
        q = Params(note * 256, 64 * 256, 0x7F00, 0, args.bright, 0, 100 * 256)
        rows.append(
            (
                note,
                440 * 2 ** ((note - 69) / 12),
                q.N,
                float(q.delta),
                float(q.a),
                q.cents_error(),
            )
        )
    worst = max(rows, key=lambda r: abs(r[5]))
    print(
        f"bright word {args.bright:#x}: worst {worst[5]:+.3f} cents at note {worst[0]}"
    )
    for lo, hi in (
        (0, 7),
        (7, 60),
        (60, 84),
        (84, 96),
        (96, 108),
        (108, 120),
        (120, 128),
    ):
        sel = [r[5] for r in rows if lo <= r[0] < hi]
        print(
            f"  notes {lo:3d}..{hi - 1:3d}: max |error| {max(abs(v) for v in sel):.3f} cents"
        )
    if args.out:
        with open(args.out, "w") as fh:
            fh.write("note,hz,N,delta,a,cents\n")
            for r in rows:
                fh.write(",".join(str(v) for v in r) + "\n")
    return 0


def cmd_measure(args: argparse.Namespace) -> int:
    """Render each note with the float32 model (long decay, bright) and measure the fundamental."""
    print("note   hz        analytic   measured (cents)")
    for note in args.notes:
        want = 440.0 * 2 ** ((note - 69) / 12)
        period = FS / want
        q = Params(note * 256, 64 * 256, 0x7F00, 0, 0x7F00, 0x7000, 100 * 256)
        s = Pluck()
        s.note_on(q)
        n_blocks = int(math.ceil(max(3200, 40 * period) / 32)) + 4
        y = np.concatenate([s.block(q) for _ in range(n_blocks)]).astype(np.float64)
        y = y[int(4 * period) :]
        got = f0_from_crossings(y, want)
        print(
            f"{note:4d} {want:9.2f} {q.cents_error():+9.3f} {1200 * math.log2(got / want):+9.3f}"
        )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    t = sub.add_parser("tuning")
    t.add_argument("--bright", type=lambda s: int(s, 0), default=0x4000)
    t.add_argument("--out")
    t.set_defaults(fn=cmd_tuning)
    m = sub.add_parser("measure")
    m.add_argument(
        "notes",
        nargs="*",
        type=int,
        default=[12, 24, 36, 48, 60, 72, 84, 96, 100, 104, 108],
    )
    m.set_defaults(fn=cmd_measure)
    a = ap.parse_args()
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
