import FFT from 'fft.js';
import {
  FFT_SIZE,
  MIN_PITCH_CYCLES,
  SPECTRUM_FLOOR,
  dbfs,
  frequencyAt,
  type AnalyzerFrame,
  type AnalyzerInput,
  type Meter,
  type Stereo,
} from './frame';

const SPECTRUM_POINTS = 384;

function measure(samples: Float32Array): Meter {
  let peak = 0;
  let sum = 0;
  for (const sample of samples) {
    peak = Math.max(peak, Math.abs(sample));
    sum += sample * sample;
  }
  return { peak, rms: Math.sqrt(sum / samples.length), clipped: peak >= 1 };
}

function risingEdge(samples: Float32Array, first: number, last: number) {
  for (let index = last; index >= first; index--) {
    if (samples[index - 1] < 0 && samples[index] >= 0) return index;
  }
  return -1;
}

// Select a prominent periodic peak. A lower peak can supersede the dominant one when
// both are strong enough and separated by an integer harmonic.
function estimateFundamental(magnitudes: Float32Array, rate: number) {
  const binHz = rate / FFT_SIZE;
  const last = magnitudes.length - 2;
  let strongest = 1;
  let power = 0;
  for (let bin = 1; bin <= last; bin++) {
    const magnitude = magnitudes[bin];
    power += magnitude * magnitude;
    if (magnitude > magnitudes[strongest]) strongest = bin;
  }
  const peak = magnitudes[strongest];
  if (strongest < MIN_PITCH_CYCLES || peak < 0.001 || peak * peak < power * 0.12) return null;

  const interpolate = (bin: number) => {
    const before = Math.log(Math.max(1e-12, magnitudes[bin - 1]));
    const center = Math.log(Math.max(1e-12, magnitudes[bin]));
    const after = Math.log(Math.max(1e-12, magnitudes[bin + 1]));
    const curvature = before - 2 * center + after;
    const offset = curvature < 0 ? (0.5 * (before - after)) / curvature : 0;
    return bin + Math.max(-0.5, Math.min(0.5, offset));
  };

  const dominant = interpolate(strongest);
  if (dominant < MIN_PITCH_CYCLES) return null;
  for (let bin = MIN_PITCH_CYCLES; bin <= strongest / 2 + 1; bin++) {
    if (
      magnitudes[bin] < peak * 0.15 ||
      magnitudes[bin] <= magnitudes[bin - 1] ||
      magnitudes[bin] < magnitudes[bin + 1]
    )
      continue;
    const candidate = interpolate(bin);
    const harmonic = Math.round(dominant / candidate);
    if (
      candidate >= MIN_PITCH_CYCLES &&
      harmonic >= 2 &&
      harmonic <= 8 &&
      Math.abs(dominant - harmonic * candidate) <= 1
    )
      return candidate * binHz;
  }
  return dominant * binHz;
}

function logSpectrum(magnitudes: Float32Array, rate: number) {
  const display = new Float32Array(SPECTRUM_POINTS);
  const binHz = rate / FFT_SIZE;
  for (let point = 0; point < display.length; point++) {
    const axis = point / (display.length - 1);
    const bin = frequencyAt(axis, rate) / binHz;
    const lower = Math.floor(bin);
    let magnitude =
      magnitudes[lower] + ((magnitudes[lower + 1] ?? magnitudes[lower]) - magnitudes[lower]) * (bin - lower);
    // Preserve narrow peaks when a plot point covers several high-frequency bins.
    const half = 0.5 / (display.length - 1);
    const first = Math.ceil(frequencyAt(Math.max(0, axis - half), rate) / binHz);
    const last = Math.floor(frequencyAt(Math.min(1, axis + half), rate) / binHz);
    for (let covered = first; covered <= last; covered++) magnitude = Math.max(magnitude, magnitudes[covered]);
    display[point] = Math.max(SPECTRUM_FLOOR, dbfs(magnitude));
  }
  return display;
}

export function createAnalyzerMeasurement() {
  const fft = new FFT(FFT_SIZE);
  const input = new Float32Array(FFT_SIZE);
  const output = fft.createComplexArray();
  const magnitudes = new Float32Array(FFT_SIZE / 2 + 1);
  const window = Float32Array.from(
    { length: FFT_SIZE },
    (_, index) => 0.5 - 0.5 * Math.cos((2 * Math.PI * index) / (FFT_SIZE - 1)),
  );
  const windowGain = window.reduce((sum, sample) => sum + sample, 0);
  let revision = -1;
  let rate = 0;
  let held: Stereo<Float32Array> | null = null;
  let holdRevision = -1;

  function spectrumOf(samples: Float32Array) {
    const mean = samples.reduce((sum, sample) => sum + sample, 0) / FFT_SIZE;
    for (let index = 0; index < FFT_SIZE; index++) {
      input[index] = (samples[index] - mean) * window[index];
    }
    fft.realTransform(output, input);
    for (let bin = 0; bin < magnitudes.length; bin++) {
      const gain = bin === 0 || bin === FFT_SIZE / 2 ? 1 : 2;
      magnitudes[bin] = (gain * Math.hypot(output[bin * 2], output[bin * 2 + 1])) / windowGain;
    }
    return { plot: logSpectrum(magnitudes, rate), pitch: estimateFundamental(magnitudes, rate) };
  }

  return function analyze(block: AnalyzerInput): AnalyzerFrame {
    if (revision !== block.revision || rate !== block.rate) {
      revision = block.revision;
      rate = block.rate;
      held = null;
    }
    if (holdRevision !== block.holdRevision || !block.hold) {
      holdRevision = block.holdRevision;
      held = null;
    }

    // Each read is contiguous internally. Adjacent reads can overlap or skip samples.
    const count = Math.min(FFT_SIZE, block.left.length, block.right.length);
    const left = block.left.subarray(block.left.length - count);
    const right = block.right.subarray(block.right.length - count);
    const meters: Stereo<Meter> = [measure(left), measure(right)];
    const scopeSize = Math.min(count, Math.round(rate * 0.01));
    const latestScope = count - scopeSize;
    const earliestEdge = Math.max(1, latestScope - scopeSize);
    let edge = -1;
    if (block.trigger && earliestEdge <= latestScope) {
      if (meters[0].peak >= 0.001) edge = risingEdge(left, earliestEdge, latestScope);
      if (edge < 0 && meters[1].peak >= 0.001) edge = risingEdge(right, earliestEdge, latestScope);
    }
    const start = edge < 0 ? latestScope : edge;
    const scope: Stereo<Float32Array> = [left.slice(start, start + scopeSize), right.slice(start, start + scopeSize)];

    let spectrum: Stereo<Float32Array> | null = null;
    let fundamental: Stereo<number | null> = [null, null];
    if (count === FFT_SIZE) {
      const leftSpectrum = spectrumOf(left);
      const rightSpectrum = spectrumOf(right);
      spectrum = [leftSpectrum.plot, rightSpectrum.plot];
      fundamental = [leftSpectrum.pitch, rightSpectrum.pitch];
      if (block.hold) {
        held ??= [
          new Float32Array(SPECTRUM_POINTS).fill(SPECTRUM_FLOOR),
          new Float32Array(SPECTRUM_POINTS).fill(SPECTRUM_FLOOR),
        ];
        for (const channel of [0, 1] as const) {
          for (let point = 0; point < SPECTRUM_POINTS; point++) {
            held[channel][point] = Math.max(held[channel][point], spectrum[channel][point]);
          }
        }
      }
    }
    return {
      rate,
      meters,
      scope,
      triggered: edge >= 0,
      spectrum,
      heldSpectrum: held && [held[0].slice(), held[1].slice()],
      fundamental,
    };
  };
}
