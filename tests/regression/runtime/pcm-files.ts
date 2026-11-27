import fs from 'node:fs';
import { expect } from 'vitest';
import { record } from './artifacts.ts';
import { addedFactor, comparePcmAligned } from './pcm-compare.ts';

/** The samples of a completed capture: not empty and all finite (the FX send scenarios read them). */
export function readPcm(file: string): Float32Array {
  const samples = readFloatWav(file);
  if (!samples.length) throw new Error(`${file}: empty PCM`);
  if (!samples.every(Number.isFinite)) throw new Error(`${file}: PCM contains nonfinite samples`);
  return samples;
}

/** The sample count, finiteness and peak of a capture. */
export function pcmStats(file: string) {
  const samples = readPcm(file);
  return { samples: samples.length, finite: true, peak: peak(samples) };
}

/** The factor a send adds to the dry capture `base` in the capture `out` (`addedFactor`), refused when the window cannot be fitted. */
export function addedFactorOf(base: string, out: string): number {
  const [dry, wet] = [readPcm(base), readPcm(out)];
  const n = Math.min(dry.length, wet.length) - 1200;
  if (n <= 0) throw new Error('added-factor requires more than 1200 samples per WAV');
  if (!dry.subarray(600, 600 + n).some((v) => v !== 0))
    throw new Error('added-factor requires a non-silent reference fit window');
  const value = addedFactor(dry, wet);
  if (!Number.isFinite(value)) throw new Error('added-factor produced a nonfinite result');
  return value;
}

export interface PcmMetrics {
  ok: boolean;
  identical: boolean;
  rms_db: number | null;
  rms_base: number;
  shift: number | null;
}
/** The metrics of a render against the reference scaled by `gain`, from the onset on (see `comparePcmAligned`). */
export async function pcmMetrics(
  base: string,
  sample: string,
  gain: number,
  artifacts: string,
  tag: string,
  relBound?: number,
): Promise<PcmMetrics> {
  const metrics = comparePcmAligned(readFloatWav(base), readFloatWav(sample), gain, relBound) as unknown as PcmMetrics;
  record(artifacts, `pcm-${tag}`, metrics);
  return metrics;
}

/**
 * Asserts that a render equals the reference scaled by `gain`. The verdict (`ok` of `comparePcmAligned`: the largest error within
 * `relBound` of the scaled reference peak, 2**-20 when unset) is the only error rule; this adds audibility, the gain
 * in dB and the onset shift (shifts count interleaved samples; the tolerance is 128 audio frames).
 */
export async function compare(
  base: string,
  sample: string,
  gain: number,
  artifacts: string,
  tag: string,
  identical = false,
  relBound?: number,
): Promise<PcmMetrics> {
  const metrics = await pcmMetrics(base, sample, gain, artifacts, tag, relBound);
  expect(metrics.rms_base, 'reference has sound').toBeGreaterThan(0);
  expect(metrics.ok, 'PCM equals the scaled reference within the error bound').toBe(true);
  expect(metrics.rms_db).not.toBeNull();
  expect(Math.abs(metrics.rms_db! - 20 * Math.log10(gain))).toBeLessThan(0.001);
  expect(metrics.shift).not.toBeNull();
  expect(Math.abs(metrics.shift!) / 2).toBeLessThanOrEqual(128);
  if (identical) expect(metrics.identical, 'dry PCM is bit-identical from onset').toBe(true);
  return metrics;
}

/** Interleaved samples of an IEEE-float WAV (format 3). */
export function readFloatWav(file: string): Float32Array {
  const d = fs.readFileSync(file);
  if (d.toString('latin1', 0, 4) !== 'RIFF' || d.toString('latin1', 8, 12) !== 'WAVE')
    throw new Error(`${file}: not a WAV`);
  let pos = 12;
  let format = 0;
  while (pos + 8 <= d.length) {
    const tag = d.toString('latin1', pos, pos + 4);
    const size = d.readUInt32LE(pos + 4);
    if (tag === 'fmt ') format = d.readUInt16LE(pos + 8);
    else if (tag === 'data') {
      if (format !== 3) throw new Error(`${file}: expected an IEEE float WAV, format ${format}`);
      const out = new Float32Array(Math.floor(Math.min(size, d.length - pos - 8) / 4));
      for (let i = 0; i < out.length; i++) out[i] = d.readFloatLE(pos + 8 + 4 * i);
      return out;
    }
    pos += 8 + size + (size & 1);
  }
  throw new Error(`${file}: no data chunk`);
}

/** (onset of `run` minus onset of `ref`, number of samples that differ from the onsets on over the common length); the onset is the first sample above 1e-6. */
export function alignedCompare(ref: Float32Array, run: Float32Array) {
  const onset = (x: Float32Array) => x.findIndex((v) => Math.abs(v) > 1e-6);
  const a = onset(ref);
  const b = onset(run);
  if (a < 0 || b < 0) return { shift: null, differ: -1 };
  const n = Math.min(ref.length - a, run.length - b);
  let differ = 0;
  for (let i = 0; i < n; i++) if (ref[a + i] !== run[b + i]) differ++;
  return { shift: b - a, differ };
}

/** Equal, or equal sample for sample from the onsets on with an even onset shift of at most `maxShift` samples. */
export function pcmMatches(ref: Float32Array, run: Float32Array, maxShift: number) {
  const exact = ref.length === run.length && ref.every((v, i) => v === run[i]);
  const { shift, differ } = alignedCompare(ref, run);
  const ok = exact || (shift !== null && shift % 2 === 0 && Math.abs(shift) <= maxShift && differ === 0);
  return {
    ok,
    detail: `${run.length} samples, exact ${exact}, onset shift ${shift}, ${differ} differ after alignment`,
  };
}

export const peak = (x: Float32Array): number => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0);

/** RMS of each full window of `win` samples. */
export function windowRms(samples: Float32Array, win = 4800): number[] {
  const out: number[] = [];
  for (let i = 0; i + win < samples.length; i += win) {
    let sum = 0;
    for (let k = i; k < i + win; k++) sum += samples[k]! * samples[k]!;
    out.push(Math.sqrt(sum / win));
  }
  return out;
}

/** The onset (first sample above 1e-6, -1 for none) and the little-endian float32 bytes of `n` interleaved samples from it (Python `pcm_pin`). */
export function pcmWindow(pcm: Float32Array, n: number): { onset: number; window: Buffer } {
  const onset = pcm.findIndex((x) => Math.abs(x) > 1e-6);
  const part = onset >= 0 ? pcm.subarray(onset, onset + n) : pcm.subarray(0, 0);
  const window = Buffer.alloc(part.length * 4);
  part.forEach((x, i) => window.writeFloatLE(x, 4 * i));
  return { onset, window };
}
