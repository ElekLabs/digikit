/** The pure part of the LFO shape tests: the shapes as functions of a phase, the thresholds and the phase-free fit. No harness imports, so a host test runs it (tests/regression/host/lfo-fit.test.ts). */

/** The wave numbers of the stock waves (finding 149, "Waveform names"). */
export const WAVE = { TRI: 0, SINE: 1, SQR: 2, SAW: 3, EXPO: 4, RAMP: 5, RAND: 6 } as const;
/** The first wave number the lfo service gives a shape (`RANGE.lo` of the service). */
export const FIRST_SHAPE = 7;

/**
 * Fit thresholds (see `fitShape` and tests/regression/host/lfo-fit.test.ts, which measures them on synthetic samples with the jitter and quantisation of a device read):
 * the error of the shape that made the samples stays under `OK`, the error of any other shape stays above `BAD` where the tests need a verdict, and above the true error
 * by `PAIR_MARGIN` for the pair that is closest (TRI against SINE). The stock sine is a polynomial; a true sine fits its samples with an error under `SINE_OK`.
 * Measured on synthetic reads (20 trials per shape, jitter, rounding, noise; the error leaves out the worst 4% of the samples): the true shape 0.025 at most, the closest wrong
 * shape 0.125 at least. `OK` and `BAD` lie between with a factor of 2 on the true side and 0.8 on the wrong side; a device read has more noise than the synthetic one, which is why
 * `OK` is not set closer to the wrong side.
 */
export const OK = 0.06;
export const BAD = 0.1;
export const PAIR_MARGIN = 0.02;
export const SINE_OK = 0.03;

/** The 32 samples of `tests/fixtures/modules/lfo-shape/src/module.c` (RAMP): a ramp from -31744 in steps of 2048. */
export const RAMP_SAMPLES = Array.from({ length: 32 }, (_, i) => -31744 + 2048 * i);
/** The samples of `tests/fixtures/modules/lfo-shape-b/src/module.c` (STEP): -24576 for the first half, 24576 for the second. */
export const STEP_SAMPLES = Array.from({ length: 32 }, (_, i) => (i < 16 ? -24576 : 24576));

// ---- the waves as functions of a 32-bit phase, signed Q31 (finding 149, "Output scale") --------------------------------------------------

/** The value of a 32-sample table at `phase`: the formula of the service's `records::interp` (the sample at the top 5 bits, linear with the next 15). */
export function evalTable(s: readonly number[], phase: number): number {
  const i = phase >>> 27;
  const j = (i + 1) & 31;
  const f = (phase >>> 12) & 0x7fff;
  return ((s[i]! << 16) + (((s[j]! - s[i]!) * f) << 1)) | 0;
}
export const evalTri = (phase: number): number => {
  const d = (phase + 0x40000000) >>> 0;
  const mask = d >>> 31 ? 0xffffffff : 0;
  return ((d << 1) ^ mask ^ 0x80000000) | 0;
};
export const evalSqr = (phase: number): number => (phase >>> 0 < 0x80000000 ? 0x7fffffff : -0x80000000);
export const evalSaw = (phase: number): number => (phase ^ 0x7fffffff) | 0;
export const evalRamp = (phase: number): number => (phase >>> 0 < 0x80000000 ? phase | 0 : 0);
/** The stock sine is a polynomial; the fit tolerance covers its difference to the true sine. */
export const evalSine = (phase: number): number => Math.round(Math.sin((phase / 2 ** 32) * 2 * Math.PI) * 0x7fffffff);
export const STOCK_EVAL: Record<number, (phase: number) => number> = {
  [WAVE.TRI]: evalTri,
  [WAVE.SINE]: evalSine,
  [WAVE.SQR]: evalSqr,
  [WAVE.SAW]: evalSaw,
  [WAVE.RAMP]: evalRamp,
};

// ---- a phase-free fit ----------------------------------------------------------------------------------------------------------------

export interface Sample {
  /** Emulated ms. */
  t: number;
  /** The destination word. */
  v: number;
}

export interface Fit {
  /** Root mean square difference of the normalised samples to the normalised shape, 0 to about 0.5. */
  err: number;
  /** Cycles per emulated ms and the phase (0 to 1) at the first sample of the best fit. */
  f: number;
  phi: number;
}

const PHASES = 1024;
/** The fraction of the samples (the worst fitting ones) that the error leaves out. */
const DROP_FRACTION = 0.04;
const FIT_CYCLES_PER_MS: [number, number] = [1 / 2000, 1 / 24];
const FIT_RATES = 240;
const FIT_OFFSETS = 128;
/** The refinement: the rate within +-2% of the coarse best (the coarse steps are 1.9% apart) in 0.05% steps, with 256 phase offsets. */
const REFINE_STEPS = 40;
const REFINE_STEP = 0.0005;
const REFINE_OFFSETS = 256;

/**
 * Fit the samples to `shape`: the LFO output is `base + scale * shape(phase)` with a phase that grows at a constant rate, and none of base, scale, rate or start phase is
 * known. Both curves are normalised to 0..1 by their own minimum and maximum. A coarse pass searches the rate in log steps and the start phase for the smallest root mean
 * square difference, a second pass refines the rate around the coarse best (a rate error of 1% over 10 cycles is a tenth of a cycle at the end, which would put a shape
 * with an edge off by several samples). A shape that is not the one that made the samples leaves an error above `BAD` at 6 to 14 samples per cycle over 7 or more cycles;
 * the true shape leaves the quantisation of the words, the jitter of the sample times and the few samples on an edge (under `OK`). Measured in tests/regression/host/lfo-fit.test.ts.
 */
export function fitShape(samples: readonly Sample[], shape: (phase: number) => number): Fit {
  const vs = samples.map((s) => s.v);
  const lo = Math.min(...vs);
  const hi = Math.max(...vs);
  if (hi === lo) return { err: 1, f: 0, phi: 0 };
  const u = vs.map((v) => (v - lo) / (hi - lo));
  const table = Array.from({ length: PHASES }, (_, k) => shape(Math.floor((k / PHASES) * 2 ** 32) >>> 0));
  const tlo = Math.min(...table);
  const thi = Math.max(...table);
  const e = table.map((x) => (thi === tlo ? 0 : (x - tlo) / (thi - tlo)));
  const t0 = samples[0]!.t;
  const dt = samples.map((s) => s.t - t0);
  // the error leaves out the worst `dropped` samples: a sample on an edge of SQR or SAW (a read within the jitter of the tick) is wrong by a whole swing
  const dropped = Math.floor(dt.length * DROP_FRACTION);
  const errAt = (f: number, phi: number): number => {
    let sum = 0;
    const worst = Array.from({ length: dropped }, () => 0);
    for (let n = 0; n < dt.length; n++) {
      const cycle = phi + f * dt[n]!;
      const k = Math.floor((cycle - Math.floor(cycle)) * PHASES) % PHASES;
      const d = u[n]! - e[k]!;
      let sq = d * d;
      for (let j = 0; j < dropped; j++)
        if (sq > worst[j]!) {
          const keep = worst[j]!;
          worst[j] = sq;
          sq = keep;
        }
      sum += sq;
    }
    return Math.sqrt(sum / (dt.length - dropped));
  };
  let best: Fit = { err: Infinity, f: 0, phi: 0 };
  for (let r = 0; r < FIT_RATES; r++) {
    const f = FIT_CYCLES_PER_MS[0] * (FIT_CYCLES_PER_MS[1] / FIT_CYCLES_PER_MS[0]) ** (r / (FIT_RATES - 1));
    for (let o = 0; o < FIT_OFFSETS; o++) {
      const err = errAt(f, o / FIT_OFFSETS);
      if (err < best.err) best = { err, f, phi: o / FIT_OFFSETS };
    }
  }
  const centre = best.f;
  for (let s = -REFINE_STEPS; s <= REFINE_STEPS; s++) {
    const f = centre * (1 + s * REFINE_STEP);
    for (let o = 0; o < REFINE_OFFSETS; o++) {
      const err = errAt(f, o / REFINE_OFFSETS);
      if (err < best.err) best = { err, f, phi: o / REFINE_OFFSETS };
    }
  }
  return best;
}

/** Number of times the samples cross the middle of their range: 2 per cycle for every shape of the tests. */
export function crossings(samples: readonly Sample[]): number {
  const vs = samples.map((s) => s.v);
  const mid = (Math.min(...vs) + Math.max(...vs)) / 2;
  if (Math.max(...vs) === Math.min(...vs)) return 0;
  let n = 0;
  for (let i = 1; i < vs.length; i++) if (vs[i - 1]! < mid !== vs[i]! < mid) n++;
  return n;
}

/** The values in hex and the time offsets in ms (relative to the first sample), for a failure message. */
export const describeSamples = (samples: readonly Sample[]): string => {
  const t0 = samples[0]?.t ?? 0;
  const v = samples.map((s) => (s.v >>> 0).toString(16)).join(' ');
  const t = samples.map((s) => (s.t - t0).toFixed(1)).join(' ');
  return `v: ${v} | t: ${t}`;
};

/** Peak-to-peak of the samples. */
export const swing = (samples: readonly Sample[]): number => {
  const vs = samples.map((s) => s.v);
  return Math.max(...vs) - Math.min(...vs);
};

