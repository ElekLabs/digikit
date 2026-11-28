export const FFT_SIZE = 2048;
export const SPECTRUM_FLOOR = -100;
export const MIN_FREQUENCY = 20;
export const MIN_PITCH_CYCLES = 3;

export type Stereo<T> = readonly [T, T];

export interface Meter {
  peak: number;
  rms: number;
  clipped: boolean;
}

export interface AnalyzerFrame {
  rate: number;
  meters: Stereo<Meter>;
  scope: Stereo<Float32Array>;
  triggered: boolean;
  spectrum: Stereo<Float32Array> | null;
  heldSpectrum: Stereo<Float32Array> | null;
  fundamental: Stereo<number | null>;
}

export interface EnvelopeTrace {
  levels: Float32Array;
  secondsPerWindow: number;
  duration: number;
}

export interface EnvelopeFrame {
  traces: Stereo<EnvelopeTrace>;
  duration: number;
}

export interface AnalyzerInput {
  revision: number;
  rate: number;
  left: Float32Array;
  right: Float32Array;
  trigger: boolean;
  hold: boolean;
  holdRevision: number;
}

export function dbfs(value: number) {
  return value > 0 ? 20 * Math.log10(value) : -Infinity;
}

export function frequencyAt(fraction: number, rate: number) {
  return MIN_FREQUENCY * (rate / (2 * MIN_FREQUENCY)) ** fraction;
}

export interface AnalyzerResult {
  revision: number;
  holdRevision: number;
  frame: AnalyzerFrame;
}
