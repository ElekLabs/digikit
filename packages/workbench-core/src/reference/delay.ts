/** Stock Delay reference analysis. Positions are measured in PCM frames, never host time. */
import { delayReference } from './dt2-stock-delay.ts';

interface Pcm {
  rate: number;
  channels: number;
  samples: readonly number[];
}

function envelope(pcm: Pcm): Float64Array {
  const frames = pcm.samples.length / pcm.channels;
  if (pcm.rate !== delayReference.rate || pcm.channels !== 2 || !Number.isInteger(frames) || frames < pcm.rate)
    throw new Error('Expected at least one second of 48 kHz stereo PCM.');
  const sums = new Float64Array(frames + 1);
  for (let k = 0; k < frames; k++) {
    const l = pcm.samples[2 * k]!;
    const r = pcm.samples[2 * k + 1]!;
    if (!Number.isFinite(l) || !Number.isFinite(r) || Math.max(Math.abs(l), Math.abs(r)) >= 1)
      throw new Error('Reference recording has clipped or non-finite samples.');
    sums[k + 1] = sums[k]! + (l * l + r * r) / 2;
  }
  const half = 120; // 5 ms RMS
  return Float64Array.from({ length: frames }, (_, i) => {
    const start = Math.max(0, i - half);
    const end = Math.min(frames, i + half);
    return Math.sqrt(Math.max(0, sums[end]! - sums[start]!) / (end - start));
  });
}

const frames = (ms: number) => Math.round(ms * 48);
const db = (v: number) => 20 * Math.log10(Math.max(v, 1e-12));

function top(trace: Float64Array, a: number, b: number): number {
  if (a < 0 || b > trace.length || a >= b) throw new Error('The capture ended before the echo window.');
  let k = a;
  for (let i = a + 1; i < b; i++) if (trace[i]! > trace[k]!) k = i;
  return k;
}

/** Require a short 1 kHz direct arrival, one dry capture and five distinct wet returns. */
export function measureDelay(dry: Pcm, wet: Pcm) {
  const d = envelope(dry);
  const w = envelope(wet);
  const direct = top(d, 0, d.length);
  const peak = d[direct]!;
  if (peak < 1e-4 || direct < frames(40)) throw new Error('No isolated probe after the capture pre-roll.');
  const active = d.findIndex((x, i) => i > direct && x < peak * 0.04) - d.findIndex((x) => x >= peak * 0.04);
  // Estimate pitch from several cycles around the highest-energy part of the pulse.
  const crossings = [];
  for (let i = direct - 240; i < direct + 240; i++) {
    const a = dry.samples[2 * i]!;
    const b = dry.samples[2 * i + 2]!;
    if (a <= 0 && b > 0) crossings.push(i - a / (b - a));
  }
  const toneHz = (48_000 * (crossings.length - 1)) / (crossings.at(-1)! - crossings[0]!);
  if (!Number.isFinite(toneHz) || Math.abs(toneHz - 1000) > 35 || active < frames(12) || active > frames(40))
    throw new Error('Dry recording is not the short 1 kHz probe. Check the assigned sample, tuning and envelope.');
  const first = w.findIndex((x) => x > peak * 0.18);
  if (first < frames(40)) throw new Error('Wet recording has no clean pre-roll or direct arrival.');
  const wetDirect = top(w, first, first + frames(25));
  const firstEcho = top(w, wetDirect + frames(46), wetDirect + frames(84));
  const spacing = firstEcho - wetDirect;
  if (d[top(d, direct + frames(46), direct + frames(84))]! > peak * 0.025)
    throw new Error('The dry pass contains a return. Check MIDI input and the project sends.');
  if (Math.abs(db(w[wetDirect]! / peak)) > 2 || w[firstEcho]! < peak * 0.12)
    throw new Error('The wet pass has a different direct level or no Delay return.');
  const positions = [firstEcho];
  for (let i = 1; i < 5; i++) {
    const at = firstEcho + spacing * i;
    positions.push(top(w, at - frames(4), at + frames(4)));
  }
  const levels = positions.map((p) => db(w[p]!));
  if (levels.some((v, i) => i > 0 && levels[i - 1]! - v < 2))
    throw new Error('The five returns do not decay as expected.');
  const spacingMs = (positions[4]! - firstEcho) / (4 * 48);
  const lossDbPerRepeat = -levels.reduce((s, level, i) => s + (i - 2) * level, 0) / 10;
  const passed =
    spacingMs >= delayReference.spacingMs[0] &&
    spacingMs <= delayReference.spacingMs[1] &&
    lossDbPerRepeat >= delayReference.lossDb[0] &&
    lossDbPerRepeat <= delayReference.lossDb[1];
  return {
    passed,
    toneHz,
    durationMs: active / 48,
    spacingMs,
    lossDbPerRepeat,
    firstReturnDbfs: levels[0]!,
    levelsDbfs: levels,
  };
}

type Timing = Pick<ReturnType<typeof measureDelay>, 'spacingMs' | 'lossDbPerRepeat'>;

export function compareDelay(baseline: Timing, candidate: Timing) {
  if (![baseline, candidate].every((v) => Number.isFinite(v.spacingMs) && Number.isFinite(v.lossDbPerRepeat)))
    throw new Error('Both reports must contain finite spacing and decay measurements.');
  const deltaSpacingMs = candidate.spacingMs - baseline.spacingMs;
  const deltaLossDbPerRepeat = candidate.lossDbPerRepeat - baseline.lossDbPerRepeat;
  return {
    passed:
      Math.abs(deltaSpacingMs) <= delayReference.pairSpacingMs &&
      Math.abs(deltaLossDbPerRepeat) <= delayReference.pairLossDb,
    deltaSpacingMs,
    deltaLossDbPerRepeat,
    toleranceSpacingMs: delayReference.pairSpacingMs,
    toleranceLossDbPerRepeat: delayReference.pairLossDb,
  };
}
