import { expect, test } from 'vitest';
import {
  BAD,
  evalRamp,
  evalSaw,
  evalSine,
  evalSqr,
  evalTable,
  evalTri,
  fitShape,
  OK,
  PAIR_MARGIN,
  RAMP_SAMPLES,
  SINE_OK,
  type Sample,
} from '../../../modules/core/runtime/tests/lfo/lfo-fit.ts';

/**
 * The fit of the LFO shape tests, on synthetic reads with what a device read has: a rate of 6 to 14 cycles per second (the window of `tuneSpeed`), a 12 ms read gap,
 * time jitter of the 0.67 ms tick, words rounded to integers, one unit of noise, and a random base, swing (0x500 to 0x2000) and start phase.
 */
// the STEP fixture table is a square wave (the same shape as SQR), so it is not listed
const SHAPES: Record<string, (phase: number) => number> = {
  TRI: evalTri,
  SINE: evalSine,
  SQR: evalSqr,
  SAW: evalSaw,
  RAMP: evalRamp,
  TABLE: (p) => evalTable(RAMP_SAMPLES, p),
};

let seed = 12345;
const random = () => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  return seed / 2 ** 32;
};

function synth(shape: (phase: number) => number): Sample[] {
  const rate = (6 + 8 * random()) / 1000; // cycles per ms
  const phi = random();
  const swing = 0x500 + random() * (0x2000 - 0x500);
  const base = 0x3000 + random() * 0x1000;
  return Array.from({ length: 100 }, (_, n) => {
    const t = n * 12;
    const cycle = phi + rate * (t + (random() - 0.5) * 0.67);
    const phase = Math.floor((cycle - Math.floor(cycle)) * 2 ** 32) >>> 0;
    return { t, v: Math.round(base + (swing * shape(phase)) / 2 ** 32 + (random() - 0.5) * 2) };
  });
}

const TRIALS = 8;

test('the true shape fits under OK and every other shape stays above the margin', () => {
  const worst: Record<string, number> = {};
  let closest = Infinity;
  for (const [name, shape] of Object.entries(SHAPES)) {
    for (let trial = 0; trial < TRIALS; trial++) {
      const samples = synth(shape);
      const own = fitShape(samples, shape);
      worst[name] = Math.max(worst[name] ?? 0, own.err);
      expect(own.err, `${name} trial ${trial} fits its own samples`).toBeLessThan(name === 'SINE' ? SINE_OK : OK);
      for (const [other, otherShape] of Object.entries(SHAPES)) {
        if (other === name) continue;
        const err = fitShape(samples, otherShape).err;
        const pair = [name, other];
        const close = pair.includes('TRI') && pair.includes('SINE');
        expect(err, `${name} samples against ${other} (trial ${trial}): own ${own.err.toFixed(3)}`).toBeGreaterThan(
          own.err + (close ? PAIR_MARGIN : 0),
        );
        if (!close) closest = Math.min(closest, err);
      }
    }
  }
  // the verdict thresholds of the device tests: a shape that is not the one that made the samples stays above BAD
  expect(
    closest,
    `closest wrong shape (excluding TRI against SINE); worst own errors ${JSON.stringify(worst)}`,
  ).toBeGreaterThan(BAD);
}, 300_000);
