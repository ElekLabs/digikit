import { describe, expect, it } from 'vitest';
import { compareDelay, measureDelay } from '../src/reference/delay.ts';

function signal(pulses: readonly [ms: number, level: number][], hz = 1000) {
  const rate = 48_000;
  const samples = Array<number>(rate * 3).fill(0);
  for (const [ms, level] of pulses) {
    for (let i = 0; i < 1200; i++) {
      const v = level * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / 1199)) * Math.sin((2 * Math.PI * hz * i) / rate);
      const at = 2 * (Math.round(ms * 48) + i);
      samples[at] = v;
      samples[at + 1] = 0.8 * v;
    }
  }
  return { rate, channels: 2, samples };
}

const dry = signal([[200, 0.1]]);
const wet = (spacing: number, gain = 0.5) =>
  signal([
    [256, 0.1],
    ...Array.from({ length: 5 }, (_, i): [number, number] => [256 + spacing * (i + 1), 0.08 * gain ** i]),
  ]);

describe('stock Delay reference', () => {
  it('measures in audio frames, independently of when a capture started', () => {
    const shape = measureDelay(dry, wet(63.5));
    expect(shape.passed).toBe(true);
    expect(shape.toneHz).toBeCloseTo(1000, 2);
    expect(shape.spacingMs).toBeCloseTo(63.5, 2);
    expect(shape.lossDbPerRepeat).toBeCloseTo(6.02, 2);
  });

  it('refuses missing wet data, a wet dry take and the wrong assigned sound', () => {
    expect(() => measureDelay(dry, dry)).toThrow('no Delay return');
    expect(() =>
      measureDelay(
        signal([
          [200, 0.1],
          [264, 0.05],
        ]),
        wet(64),
      ),
    ).toThrow('dry pass contains a return');
    expect(() => measureDelay(signal([[200, 0.1]], 440), wet(64))).toThrow('not the short 1 kHz probe');
  });

  it('reports out-of-range settings and compares passing takes numerically', () => {
    expect(measureDelay(dry, wet(64, 0.7)).passed).toBe(false);
    const base = measureDelay(dry, wet(64));
    expect(compareDelay(base, measureDelay(dry, wet(65.5, 0.49))).passed).toBe(true);
    expect(compareDelay(base, measureDelay(dry, wet(66.5))).passed).toBe(false);
    expect(compareDelay(base, measureDelay(dry, wet(64, 0.43))).passed).toBe(false);
  });
});
