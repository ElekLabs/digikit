import { measure, type FingerprintV2 } from './pcmpin.ts';
import { BAND_WINDOW_FRAMES } from './fingerprint.ts';

export interface AudioPcm {
  readonly rate: number;
  readonly channels: number;
  readonly samples: readonly number[];
}

export interface AudioSummary {
  fingerprint: FingerprintV2;
  rmsDbfs: number | null;
  fullScaleFrames: number;
  monoRmsDbfs: number | null;
  monoDeltaDb: number | null;
  stereoCorrelation: number | null;
  periodicityChecked: boolean;
  spectrumFrames: number;
}

function db(amplitude: number): number | null {
  return amplitude > 0 ? 20 * Math.log10(amplitude) : null;
}

/** Use the rig's fingerprint. Account for both channels separately before folding to mono. */
export function summarizeAudio({ samples, channels, rate }: AudioPcm): AudioSummary {
  if (
    ![1, 2].includes(channels) ||
    !Number.isInteger(rate) ||
    rate < 100 ||
    !samples.length ||
    samples.length % channels ||
    samples.some((n) => !Number.isFinite(n))
  )
    throw new Error('Expected nonempty finite mono or stereo PCM with complete frames.');
  const frames = samples.length / channels;
  // The fingerprint's whole-signal pitch estimator uses an 8x zero-padded FFT.
  // Don't run it on a full-length mix. The level envelope still covers every frame.
  const periodicityChecked = frames >= 2048 && frames <= 32768;
  const { fp: fingerprint, mono } = measure(samples, { channels, rate, periodic: periodicityChecked });
  let sumL = 0,
    sumR = 0,
    squareL = 0,
    squareR = 0,
    cross = 0,
    sumMono = 0;
  let fullScaleFrames = 0;
  for (let i = 0; i < frames; i++) {
    const l = samples[i * channels]!;
    const r = channels === 2 ? samples[i * channels + 1]! : 0;
    sumL += l;
    sumR += r;
    squareL += l * l;
    squareR += r * r;
    cross += l * r;
    sumMono += mono[i]! ** 2;
    if (Math.max(Math.abs(l), Math.abs(r)) >= 1) fullScaleFrames++;
  }
  const level = db(Math.sqrt((squareL + squareR) / samples.length));
  const fold = db(Math.sqrt(sumMono / frames));
  const varianceL = Math.max(0, squareL - sumL ** 2 / frames);
  const varianceR = Math.max(0, squareR - sumR ** 2 / frames);
  const correlation = Math.sqrt(varianceL * varianceR);
  return {
    fingerprint,
    rmsDbfs: level,
    fullScaleFrames,
    monoRmsDbfs: fold,
    monoDeltaDb: level === null || fold === null ? null : fold - level,
    stereoCorrelation:
      channels === 2 && correlation > 0
        ? Math.min(1, Math.max(-1, (cross - (sumL * sumR) / frames) / correlation))
        : null,
    periodicityChecked,
    spectrumFrames: Math.min(frames, BAND_WINDOW_FRAMES),
  };
}

export function audioChannel(pcm: AudioPcm, channel: number): number[] {
  return Array.from(
    { length: pcm.samples.length / pcm.channels },
    (_, frame) => pcm.samples[frame * pcm.channels + channel]!,
  );
}

/** Paired samples in native units. Call only after checking rate, channels and frame count. */
export function comparePcm(a: AudioPcm, b: AudioPcm) {
  return Array.from({ length: a.channels }, (_, channel) => {
    let maxAbs = 0;
    let squared = 0;
    let changedSamples = 0;
    for (let i = channel; i < a.samples.length; i += a.channels) {
      const d = b.samples[i]! - a.samples[i]!;
      if (d !== 0) changedSamples++;
      maxAbs = Math.max(maxAbs, Math.abs(d));
      squared += d * d;
    }
    return { channel: channel + 1, changedSamples, maxAbs, rms: Math.sqrt(squared / (a.samples.length / a.channels)) };
  });
}
