import { describe, expect, it } from 'vitest';
import { dbfs, FFT_SIZE, frequencyAt, SPECTRUM_FLOOR, type AnalyzerInput } from '../src/analyzer-frame';
import { createAnalyzerMeasurement } from '../src/analyzer-measurement';

const RATE = 48000;

function tone(frequency: number, amplitude: number, rate = RATE) {
  return Float32Array.from(
    { length: FFT_SIZE },
    (_, index) => amplitude * Math.sin((2 * Math.PI * index * frequency) / rate),
  );
}

function block(left: Float32Array, right = left, options: Partial<AnalyzerInput> = {}): AnalyzerInput {
  return {
    left,
    right,
    rate: RATE,
    revision: 1,
    trigger: true,
    hold: false,
    holdRevision: 0,
    ...options,
  };
}

describe('analyzer measurements', () => {
  it('keeps stereo levels and pitches independent', () => {
    const analyze = createAnalyzerMeasurement();
    const frame = analyze(block(tone(440, 0.5), tone(1234, 0.25)));
    expect(frame.fundamental[0]).toBeCloseTo(440, 0);
    expect(frame.fundamental[1]).toBeCloseTo(1234, 0);
    expect(dbfs(frame.meters[0].peak)).toBeCloseTo(-6.02);
    expect(dbfs(frame.meters[1].rms)).toBeCloseTo(-15.05, 1);
    expect(frame.meters[0].clipped).toBe(false);

    const spectrum = frame.spectrum![1];
    const peak = spectrum.indexOf(Math.max(...spectrum));
    expect(frequencyAt(peak / (spectrum.length - 1), RATE)).toBeGreaterThan(1200);
    expect(frequencyAt(peak / (spectrum.length - 1), RATE)).toBeLessThan(1260);
  });

  it('triggers a common stereo time window and uses the right channel when the left is quiet', () => {
    const analyze = createAnalyzerMeasurement();
    const right = tone(440, 0.5);
    const left = new Float32Array(FFT_SIZE);
    const triggered = analyze(block(left, right));
    expect(triggered.triggered).toBe(true);
    expect(triggered.scope[1][0]).toBeGreaterThanOrEqual(0);
    expect(triggered.scope[1][0]).toBeLessThan(0.03);
    expect(triggered.scope[1][1]).toBeGreaterThan(triggered.scope[1][0]);
    const freeRunning = analyze(block(left, right, { trigger: false }));
    expect(freeRunning.triggered).toBe(false);
    expect(freeRunning.scope[1].at(-1)).toBe(right.at(-1));
  });

  it('reports clipping and avoids assigning a pitch to DC, silence, or weak input', () => {
    const analyze = createAnalyzerMeasurement();
    const silence = new Float32Array(FFT_SIZE);
    const clipped = analyze(block(new Float32Array(FFT_SIZE).fill(1), silence));
    expect(clipped.meters[0]).toEqual({ clipped: true, peak: 1, rms: 1 });
    expect(dbfs(clipped.meters[1].rms)).toBe(-Infinity);
    expect(clipped.fundamental).toEqual([null, null]);
    expect(clipped.triggered).toBe(false);
    expect(clipped.spectrum?.[1][100]).toBe(SPECTRUM_FLOOR);
    expect(analyze(block(tone(440, 0.00001))).fundamental).toEqual([null, null]);
  });

  it('prefers a present fundamental when a harmonic is the strongest peak', () => {
    const analyze = createAnalyzerMeasurement();
    const low = tone(220, 0.15);
    const high = tone(440, 0.5);
    const mixed = Float32Array.from(low, (value, index) => value + high[index]);
    expect(analyze(block(mixed)).fundamental[0]).toBeCloseTo(220, 0);
  });

  it('avoids pitch estimates for a tone with fewer than three cycles in the capture', () => {
    const analyze = createAnalyzerMeasurement();
    expect(analyze(block(tone(35, 0.5))).fundamental).toEqual([null, null]);
    expect(analyze(block(tone(85, 0.5))).fundamental[0]).toBeCloseTo(85, 0);
  });

  it('does not stitch discontinuous captures into a spectrum', () => {
    const analyze = createAnalyzerMeasurement();
    const first = tone(440, 0.5).subarray(0, FFT_SIZE / 2);
    const second = tone(1500, 0.5).subarray(FFT_SIZE / 2);
    expect(analyze(block(first)).spectrum).toBeNull();
    expect(analyze(block(second)).spectrum).toBeNull();
    const complete = block(tone(660, 0.25));
    expect(analyze(complete).spectrum).toEqual(createAnalyzerMeasurement()(complete).spectrum);
  });

  it('holds spectral peaks and resets them on a new source, rate or hold revision', () => {
    const analyze = createAnalyzerMeasurement();
    const samples = tone(440, 0.5);
    const silence = new Float32Array(FFT_SIZE);
    const options = { hold: true };
    const loud = analyze(block(samples, samples, options));
    const held = analyze(block(silence, silence, options));
    expect(Math.max(...held.heldSpectrum![0])).toBe(Math.max(...loud.spectrum![0]));
    expect(Math.max(...held.spectrum![0])).toBe(SPECTRUM_FLOOR);
    expect(analyze(block(silence, silence, { ...options, holdRevision: 1 })).heldSpectrum?.[0][100]).toBe(
      SPECTRUM_FLOOR,
    );
    analyze(block(samples, samples, { ...options, holdRevision: 1 }));

    const next = analyze(block(silence.subarray(0, 1024), silence.subarray(0, 1024), { ...options, revision: 2 }));
    expect(next.spectrum).toBeNull();
    expect(next.heldSpectrum).toBeNull();
    const rate = 44100;
    const atRate = tone(440, 0.5, rate);
    const changed = analyze(block(atRate, atRate, { ...options, rate, revision: 2 }));
    expect(changed.fundamental[0]).toBeCloseTo(440, 0);
    expect(changed.scope[0]).toHaveLength(441);
  });
});
