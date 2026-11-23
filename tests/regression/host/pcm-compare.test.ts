import { describe, expect, test } from 'vitest';
import { comparePcm, comparePcmAligned } from '../runtime/pcm-compare.ts';

/** An attack near the onset threshold, so that the threshold must scale with the gain. */
function signal(lead: number, gain = 1): Float32Array {
  const wave = [1.5e-6, 0.125, -0.25, 0.375, -0.5, 0.25].map((v) => v * gain);
  return Float32Array.from([...Array.from({ length: lead }, () => 0), ...wave]);
}
const ref = signal(4);

// The expected figures are those of the Python `hook_ab compare --align-onset` this replaces, on the same signals.
describe('comparePcmAligned', () => {
  test('a half-gain run that starts 8 samples later passes with the shift reported', () => {
    const r = comparePcmAligned(ref, signal(12, 0.5), 0.5);
    expect(r.ok).toBe(true);
    expect(r.shift).toBe(8);
    expect(r.samples).toBe(6);
    expect(r.identical).toBe(false);
    expect(r.max_err_ulps).toBe(0);
    expect(r.rms_db).toBeCloseTo(-6.020599913279624, 9);
    expect(r.rms_base).toBeCloseTo(0.2975595178565822, 12);
  });

  test('a wrong gain fails', () => {
    const r = comparePcmAligned(ref, signal(12, 0.5), 0.6);
    expect(r.ok).toBe(false);
    expect(r.max_err_ulps).toBeCloseTo(838860.8, 1);
  });

  test('one sample off at the peak fails', () => {
    const run = signal(12, 0.5);
    run[run.length - 1] = 0.2501;
    const r = comparePcmAligned(ref, run, 0.5);
    expect(r.ok).toBe(false);
    expect(r.rms_db).toBeCloseTo(-4.706602381405386, 9);
    expect(r.max_err_ulps).toBeCloseTo(4197659, 0);
  });

  test('a run cut short is compared over the common length', () => {
    const r = comparePcmAligned(ref, signal(12, 0.5).subarray(0, 16), 0.5);
    expect(r.samples).toBe(4);
    expect(r.ok).toBe(true);
  });

  test('a silent run has no onset: no shift, the worst figure, not ok', () => {
    expect(comparePcmAligned(ref, new Float32Array(20), 0.5)).toEqual({
      shift: null,
      max_err_ulps: 1e9,
      rms_db: null,
      ok: false,
    });
  });

  test('the same signal is identical, shift 0, 0 dB', () => {
    const r = comparePcmAligned(ref, ref, 1);
    expect([r.ok, r.identical, r.shift, r.rms_db]).toEqual([true, true, 0, 0]);
  });
});

describe('comparePcm', () => {
  test('unequal lengths are never ok, whatever the error', () => {
    const r = comparePcm(signal(0), signal(0).subarray(0, 4), 1);
    expect(r.length_equal).toBe(false);
    expect(r.ok).toBe(false);
  });

  test('the bound is relative to the scaled reference peak', () => {
    const base = Float32Array.from([0, 1, -1]);
    const near = Float32Array.from([0, 1 + 2 ** -22, -1]);
    expect(comparePcm(base, near, 1, 2 ** -20).ok).toBe(true);
    expect(comparePcm(base, near, 1, 2 ** -24).ok).toBe(false);
  });
});
