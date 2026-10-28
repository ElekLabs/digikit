#!/usr/bin/env python3
"""Float32 reference of the frequency shifter (src/dsp/freqshift.s of this module), its tables and its design search.

    python <this file> table
    python <this file> rejection [--shift HZ] [--blocks N]
    python <this file> design [--seed N] [--starts N]

`Shifter` mirrors the DSP routine operation for operation in numpy float32 (one rounding per SHARC+ operation, no fused
multiply-add) and in 32-bit integers where the routine uses integers, so the routine on the concrete core and this model agree
to the last bits except where the SHARC+ conventions of FLOAT and TRUNC differ. The algorithm:

  SHIFT word v (the knob's float, -64 .. +63) -> shift in Hz   f = sign(v) KHZ (2^(|v| / SPAN) - 1)         (`hz`, `target_inc`)
  one phase accumulator of 32 bits (a turn is 2^32), increment `inc` per sample, ramped in a straight line from the last block's
  increment to this block's over the 32 samples (an integer step `d = (new - old) >> 5`), the phase advances by `inc` before each sample
  the oscillator `sinq(phase)` and `sinq(phase + a quarter turn)` (a degree-7 polynomial on the folded phase, error 1.2e-6)
  per channel, two chains of four first-order allpass sections in z^-2 (a polyphase pair: even and odd samples run in independent
  states), the chain P delayed by one sample; their outputs a (P, delayed) and b (Q) differ in phase by 90 degrees
  (0.55 degrees at worst from 30 Hz to 18 kHz, that is an image rejection of 46 dB)
  out = a cos + b sin: every frequency of the input moves up by f (down for f < 0), clamped to +-7.9 (the level bound is 8)

Section k with the coefficient c, in transposed form:  t = c x;  y = t + s;  u = c y;  s' = u - x   (H = (c - z^-1) / (1 - c z^-1) per phase).
"""

from __future__ import annotations

import argparse
import math
import sys

import numpy as np

f32 = np.float32

FS = 48000.0
# the coefficients of the two chains (section order is the order of the routine; found by `design`, band 30 Hz .. 18 kHz)
COEF_P = [0.4535710651559441, 0.8576969767436714, 0.9706778026636295, 0.996705703302151]
COEF_Q = [
    0.15003120231367387,
    0.7072623247090383,
    0.934192463256155,
    0.9878474123598983,
]
# the SHIFT mapping f = KHZ (2^(|v| / SPAN) - 1) Hz; |v| is clamped to VMAX; 2^x - 1 = x (K0 + x (K1 + x (K2 + x (K3 + x K4)))) on [0, 1)
SPAN = 5.5
KHZ = 0.63
VMAX = 64.0
EXPM1 = [
    0.6931475876323645,
    0.24020654926032228,
    0.05566028683446792,
    0.0091942072858547,
    0.0017909619145713084,
]
# sin(pi u / 2) = u (S1 + v (S3 + v (S5 + v S7))), v = u^2, u in [-1, 1] (a quarter turn is u = 1)
SIN_C = [
    1.570795141795128,
    -0.6459259622209087,
    0.0795006503514177,
    -0.004371014925405622,
]
INC_PER_HZ = 2.0**32 / FS  # the phase increment of 1 Hz
CLAMP = 7.9
QUARTER = 0x40000000
HALF = 0x80000000
BLOCK = 32


def wrap32(x: int) -> int:
    """A Python integer as a signed 32-bit integer (the DSP's fixed-point add wraps)."""
    x &= 0xFFFFFFFF
    return x - (1 << 32) if x >= 1 << 31 else x


def hz(v: float) -> float:
    """The shift in Hz of the SHIFT word v, in float64 (the model's `target_inc` is this to float32 accuracy)."""
    a = min(abs(v), VMAX)
    return math.copysign(KHZ * (2.0 ** (a / SPAN) - 1.0), v)


def v_for_hz(f: float) -> float:
    """The SHIFT word that gives a shift of f Hz (the inverse of `hz`, for |f| below its end)."""
    return math.copysign(SPAN * math.log2(abs(f) / KHZ + 1.0), f)


def target_inc(v: float) -> int:
    """The phase increment of the word v as the routine computes it (float32, truncated to an integer)."""
    a = min(abs(f32(v)), f32(VMAX))
    x = f32(a * f32(1.0 / SPAN))
    i = int(x)  # TRUNC
    fr = f32(x - f32(i))
    p = f32(f32(EXPM1[4]) * fr)
    p = f32(p + f32(EXPM1[3]))
    for c in (EXPM1[2], EXPM1[1], EXPM1[0]):
        p = f32(p * fr)
        p = f32(p + f32(c))
    q = f32(p * fr)
    big = f32(np.ldexp(q, i))  # SCALB
    m = f32(f32(np.ldexp(f32(1.0), i)) - f32(1.0))
    m = f32(big + m)
    m = f32(m * f32(KHZ))
    if f32(v) < 0:
        m = f32(-m)
    return int(f32(m * f32(INC_PER_HZ)))


def sinq(p: int) -> np.float32:
    """sin of the phase p (a signed 32-bit turn fraction) as the routine computes it: fold to +-a quarter turn, then the polynomial."""
    if (p ^ wrap32(p << 1)) < 0:  # bits 31 and 30 differ: beyond a quarter turn
        p = wrap32(HALF - p)
    u = f32(f32(p) * f32(2.0**-30))
    v = f32(u * u)
    r = f32(f32(SIN_C[3]) * v)
    r = f32(r + f32(SIN_C[2]))
    r = f32(r * v)
    r = f32(r + f32(SIN_C[1]))
    r = f32(r * v)
    r = f32(r + f32(SIN_C[0]))
    return f32(r * u)


def section(
    c: np.float32, x: np.float32, s: np.float32
) -> tuple[np.float32, np.float32]:
    """One transposed first-order allpass section: returns (y, new state)."""
    y = f32(f32(c * x) + s)
    return y, f32(f32(c * y) - x)


class Shifter:
    """The state of the effect and `block`, which processes 32 stereo frames (the dk_fx_in layout: 32 left, then 32 right)."""

    def __init__(self) -> None:
        self.phase = 0
        self.inc = 0
        # states[channel][chain][section][phase of the sample, even 0 / odd 1]
        self.s = np.zeros((2, 2, 4, 2), dtype=np.float32)
        self.carry = np.zeros(
            2, dtype=np.float32
        )  # the last output of the chain P of each channel (the one-sample delay)
        self.c32 = (
            [f32(c) for c in COEF_P],
            [f32(c) for c in COEF_Q],
        )
        self.last: dict[str, np.ndarray] = {}

    def oscillator(self, inc_new: int) -> tuple[list[np.float32], list[np.float32]]:
        d = (inc_new - self.inc) >> 5
        inc = self.inc
        sins, coss = [], []
        for _ in range(BLOCK):
            inc = wrap32(inc + d)
            self.phase = wrap32(self.phase + inc)
            sins.append(sinq(self.phase))
            coss.append(sinq(wrap32(self.phase + QUARTER)))
        self.inc = inc_new
        return sins, coss

    def chain(self, ch: int, which: int, x: np.float32, n: int) -> np.float32:
        for k in range(4):
            x, self.s[ch, which, k, n & 1] = section(
                self.c32[which][k], x, self.s[ch, which, k, n & 1]
            )
        return x

    def block(self, x: np.ndarray, v: float) -> np.ndarray:
        """64 input floats and the SHIFT word v -> 64 output floats."""
        sins, coss = self.oscillator(target_inc(v))
        out = np.zeros(64, dtype=np.float32)
        hi, lo = f32(CLAMP), f32(-CLAMP)
        a_d = np.zeros((2, BLOCK), dtype=np.float32)
        for ch in range(2):
            buf = [
                self.carry[ch]
            ]  # a_d[n] = buf[n]: the chain P's output of the sample before
            for n in range(BLOCK):
                buf.append(self.chain(ch, 0, f32(x[ch * BLOCK + n]), n))
            a_d[ch] = buf[:BLOCK]
            self.carry[ch] = buf[BLOCK]
            for n in range(BLOCK):
                b = self.chain(ch, 1, f32(x[ch * BLOCK + n]), n)
                o = f32(f32(buf[n] * coss[n]) + f32(b * sins[n]))
                out[ch * BLOCK + n] = max(min(o, hi), lo)
        return out


# ---- analysis -----------------------------------------------------------------------------------------------------------------


def cmd_table(_: argparse.Namespace) -> int:
    print("SHIFT   shift (Hz)    inc (phase units)   next step (Hz)")
    for v in (0, 0.25, 1, 2, 4, 8, 12, 16, 20, 24, 32, 40, 48, 56, 63, 64):
        print(f"{v:5.2f} {hz(v):12.4f} {target_inc(v):14d} {hz(v + 1) - hz(v):14.3f}")
    print(
        "model of target_inc against the float64 mapping (|error| in Hz, v from -64 to 63.5):"
    )
    worst = max(
        abs(target_inc(v) / INC_PER_HZ - hz(v)) for v in np.linspace(-64, 63.5, 1000)
    )
    print(f"  worst {worst:.6f} Hz")
    return 0


def response(coefs: list[float], w: np.ndarray) -> np.ndarray:
    z2 = np.exp(-2j * w)
    h = np.ones_like(z2)
    for c in coefs:
        h = h * (c - z2) / (1 - c * z2)
    return h


def image_rejection(
    fs: float, f: np.ndarray, coef_p: list[float], coef_q: list[float]
) -> np.ndarray:
    """The ratio in dB of the wanted sideband to the image of the single-sideband shifter, at the input frequencies f (exact, from the
    transfer functions of the two chains)."""
    w = 2 * np.pi * f / fs
    ha = response(coef_p, w) * np.exp(-1j * w)
    hb = response(coef_q, w)
    return np.asarray(20 * np.log10(np.abs(ha - 1j * hb) / np.abs(ha + 1j * hb)))


def cmd_rejection(args: argparse.Namespace) -> int:
    f = np.geomspace(5, 23500, 4000)
    r = image_rejection(FS, f, COEF_P, COEF_Q)
    band = (f >= 30) & (f <= 18000)
    print(
        f"image rejection from the transfer functions: worst {r[band].min():.1f} dB from 30 Hz to 18 kHz at {f[band][r[band].argmin()]:.0f} Hz"
    )
    for lo, hi in (
        (5, 30),
        (30, 100),
        (100, 1000),
        (1000, 10000),
        (10000, 18000),
        (18000, 20000),
        (20000, 23500),
    ):
        sel = (f >= lo) & (f <= hi)
        print(f"  {lo:6d} .. {hi:6d} Hz: worst {r[sel].min():5.1f} dB")
    # the model on a tone
    sh = Shifter()
    f_in = 1000.0
    n = int(args.blocks)
    v = v_for_hz(args.shift)
    t = np.arange(n * BLOCK + 1)
    sig = (0.5 * np.sin(2 * np.pi * f_in * t / FS)).astype(np.float32)
    ys = []
    for k in range(n):
        blk = sig[k * BLOCK : (k + 1) * BLOCK]
        ys.append(sh.block(np.concatenate([blk, blk]), v)[:BLOCK])
    y = np.concatenate(ys)[len(ys) * BLOCK // 4 :].astype(np.float64)
    spec = np.abs(np.fft.rfft(y * np.hanning(len(y))))
    freqs = np.fft.rfftfreq(len(y), 1 / FS)

    def peak(f0: float) -> float:
        sel = np.abs(freqs - f0) < 8 * FS / len(y)
        return float(spec[sel].max())

    f_out = f_in + hz(v)
    print(
        f"model, tone {f_in:.0f} Hz shifted by {hz(v):.3f} Hz (SHIFT {v:.3f}): peak at {f_out:.1f} Hz, image at {f_in - hz(v):.1f} Hz: {20 * np.log10(peak(f_out) / peak(f_in - hz(v))):.1f} dB down"
    )
    return 0


# ---- the design of the two chains ---------------------------------------------------------------------------------------------


def design(
    seed: int, starts: int, flo: float = 30.0, fhi: float = 18000.0
) -> tuple[float, list[float], list[float]]:
    """Search the coefficients of two four-section chains whose outputs (P delayed by a sample) differ by +90 degrees over flo .. fhi:
    Levenberg-Marquardt from random starts with Lawson weights, then a Nelder-Mead polish of the largest phase error. The constants
    above came from `--seed 2 --starts 25`, polished. Returns (worst phase error in degrees, P, Q)."""
    rng = np.random.default_rng(seed)
    f = np.concatenate([np.geomspace(flo, 1000, 300), np.linspace(1000, fhi, 700)[1:]])
    w = 2 * np.pi * f / FS
    delay = np.exp(-1j * w)

    def sig(t: np.ndarray) -> np.ndarray:
        return np.asarray(1 / (1 + np.exp(-np.clip(t, -30, 30))))

    def res(t: np.ndarray) -> np.ndarray:
        ha = response(list(sig(t[:4])), w) * delay
        hb = response(list(sig(t[4:])), w)
        return np.asarray(np.angle(hb / ha * np.exp(-0.5j * np.pi)))

    def lm(t: np.ndarray, wts: np.ndarray, iters: int) -> np.ndarray:
        lam = 1e-2
        r = res(t) * wts
        cost = float(r @ r)
        for _ in range(iters):
            jac = np.empty((len(f), len(t)))
            for k in range(len(t)):
                dt = np.zeros(len(t))
                dt[k] = 1e-6
                jac[:, k] = (res(t + dt) * wts - r) / 1e-6
            a, g = jac.T @ jac, jac.T @ r
            for _ in range(30):
                step = np.linalg.solve(a + lam * np.diag(np.diag(a) + 1e-12), -g)
                rn = res(t + step) * wts
                cn = float(rn @ rn)
                if cn < cost:
                    t, r, cost, lam = t + step, rn, cn, max(lam / 3, 1e-9)
                    break
                lam *= 4
            else:
                break
        return t

    best: tuple[float, np.ndarray] | None = None
    for _ in range(starts):
        t = np.concatenate(
            [np.sort(rng.uniform(-2, 6, 4)), np.sort(rng.uniform(-2, 6, 4))]
        )
        wts = np.ones(len(f))
        for _ in range(40):
            t = lm(t, wts, 12)
            e = np.abs(res(t))
            wts = wts * (e / e.max() + 0.02) ** 0.5
            wts /= wts.mean()
        mx = float(np.abs(res(t)).max())
        if best is None or mx < best[0]:
            best = (mx, t.copy())
    assert best is not None
    t = best[1]
    simplex = [t] + [t + np.eye(8)[i] * 0.05 for i in range(8)]
    vals = [float(np.abs(res(s)).max()) for s in simplex]
    for _ in range(3000):
        idx = np.argsort(vals)
        simplex, vals = [simplex[i] for i in idx], [vals[i] for i in idx]
        c = np.mean(simplex[:-1], axis=0)
        xr = c + (c - simplex[-1])
        fr = float(np.abs(res(xr)).max())
        if fr < vals[0]:
            xe = c + 2 * (c - simplex[-1])
            fe = float(np.abs(res(xe)).max())
            simplex[-1], vals[-1] = (xe, fe) if fe < fr else (xr, fr)
        elif fr < vals[-2]:
            simplex[-1], vals[-1] = xr, fr
        else:
            xc = c + 0.5 * (simplex[-1] - c)
            fc = float(np.abs(res(xc)).max())
            if fc < vals[-1]:
                simplex[-1], vals[-1] = xc, fc
            else:
                simplex = [simplex[0]] + [
                    simplex[0] + 0.5 * (s - simplex[0]) for s in simplex[1:]
                ]
                vals = [vals[0]] + [float(np.abs(res(s)).max()) for s in simplex[1:]]
    t = simplex[int(np.argmin(vals))]
    return (
        math.degrees(min(vals)),
        sorted(sig(t[:4]).tolist()),
        sorted(sig(t[4:]).tolist()),
    )


def cmd_design(args: argparse.Namespace) -> int:
    err, p, q = design(args.seed, args.starts)
    print(
        f"worst phase error {err:.3f} degrees ({20 * math.log10(2 / math.radians(err)):.1f} dB image rejection)"
    )
    print(f"COEF_P = {p}")
    print(f"COEF_Q = {q}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("table")
    r = sub.add_parser("rejection")
    r.add_argument("--shift", type=float, default=100.0, help="Hz")
    r.add_argument("--blocks", type=int, default=1500)
    d = sub.add_parser("design")
    d.add_argument("--seed", type=int, default=2)
    d.add_argument("--starts", type=int, default=25)
    args = ap.parse_args()
    return {"table": cmd_table, "rejection": cmd_rejection, "design": cmd_design}[
        args.cmd
    ](args)


if __name__ == "__main__":
    sys.exit(main())
