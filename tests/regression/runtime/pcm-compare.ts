/** The PCM comparison of a render against a reference: the error is within `REL_PEAK_BOUND` of the scaled reference peak. */

/** The DSP rounds several times per sample, so a tiny sample can be 3 ulps off: the bound is relative to the peak of the expected signal (about -120 dB). */
export const REL_PEAK_BOUND = 2 ** -20;

export interface PcmComparison {
  samples: number;
  length_equal: boolean;
  identical: boolean;
  rms_base: number;
  rms_test: number;
  rms_db: number | null;
  peak_ratio: number | null;
  active_samples: number;
  max_abs_err: number;
  ref_peak: number;
  err_rel_peak: number | null;
  ok: boolean;
  /** a Q31 output step is 2**-31; the float path adds one float32 ulp of the sample */
  max_err_ulps: number;
}

export interface AlignedPcmComparison extends Partial<PcmComparison> {
  /** onset of the run minus onset of the reference, in interleaved samples; null when either has no sound */
  shift: number | null;
  max_err_ulps: number;
  rms_db: number | null;
  ok: boolean;
}

const peakOf = (x: ArrayLike<number>, n: number, scale = 1): number => {
  let m = 0;
  for (let i = 0; i < n; i++) m = Math.max(m, Math.abs(scale * x[i]!));
  return m;
};
const rms = (x: ArrayLike<number>, n: number): number => {
  let sum = 0;
  for (let i = 0; i < n; i++) sum += x[i]! * x[i]!;
  return Math.sqrt(sum / n);
};

/** `test` against `gain` times `base` over the common length. */
export function comparePcm(
  base: Float32Array,
  test: Float32Array,
  gain: number,
  relBound = REL_PEAK_BOUND,
): PcmComparison {
  const n = Math.min(base.length, test.length);
  const rmsA = rms(base, n);
  const rmsB = rms(test, n);
  const peakA = peakOf(base, n);
  let identical = true;
  let active = 0;
  let maxErr = 0;
  let maxUlps = 0;
  for (let i = 0; i < n; i++) {
    const a = base[i]!;
    const b = test[i]!;
    if (a !== b) identical = false;
    if (a !== 0) active++;
    const err = Math.abs(b - gain * a);
    maxErr = Math.max(maxErr, err);
    maxUlps = Math.max(maxUlps, err / Math.max(Math.abs(a) * 2 ** -23, 2 ** -31));
  }
  const refPeak = peakOf(base, n, gain);
  const lengthEqual = base.length === test.length;
  return {
    samples: n,
    length_equal: lengthEqual,
    identical,
    rms_base: rmsA,
    rms_test: rmsB,
    rms_db: rmsA > 0 && rmsB > 0 ? 20 * Math.log10(rmsB / rmsA) : null,
    peak_ratio: peakA > 0 ? peakOf(test, n) / peakA : null,
    active_samples: active,
    max_abs_err: maxErr,
    ref_peak: refPeak,
    err_rel_peak: refPeak > 0 ? maxErr / refPeak : null,
    ok: lengthEqual && maxErr <= refPeak * relBound,
    max_err_ulps: maxUlps,
  };
}

/**
 * `run` against `ref` times `gain` from the note's onset on (the boots reach the note up to a few blocks apart): `comparePcm`'s figures over the
 * common length, and the onset shift in samples. The onset threshold of the run scales with the gain, so that a quieter run is not found later on the same attack ramp.
 */
export function comparePcmAligned(
  ref: Float32Array,
  run: Float32Array,
  gain: number,
  relBound = REL_PEAK_BOUND,
): AlignedPcmComparison {
  const onset = (x: Float32Array, threshold: number) => x.findIndex((v) => Math.abs(v) > threshold);
  const a = onset(ref, 1e-6);
  const b = onset(run, 1e-6 * Math.max(Math.abs(gain), 1e-3));
  if (a < 0 || b < 0) return { shift: null, max_err_ulps: 1e9, rms_db: null, ok: false };
  const n = Math.min(ref.length - a, run.length - b);
  return { ...comparePcm(ref.subarray(a, a + n), run.subarray(b, b + n), gain, relBound), shift: b - a };
}
