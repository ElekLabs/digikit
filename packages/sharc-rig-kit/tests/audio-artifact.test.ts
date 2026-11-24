import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { writeAudioComparison, writeAudioRender, type AudioExport } from '../src/audio-artifact.ts';
import type { RenderedCase } from '../src/render-case.ts';
import { wavBytes } from '../src/plot.ts';
import { DEVICES } from '../src/spec.ts';
import { summarizeAudio } from '../src/audio-summary.ts';
import { BAND_WINDOW_FRAMES } from '../src/fingerprint.ts';

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function fixture() {
  const dir = mkdtempSync(path.join(tmpdir(), 'dk-audio-'));
  dirs.push(dir);
  const source = (samples: number[], hash = 'a'): RenderedCase => ({
    module: 'test/sample',
    device: DEVICES[0],
    case: 'simple',
    tap: 'master-insert',
    workload: { sha256: hash.repeat(64), seed: 1, settings: {} },
    build: { rigSha256: '1'.repeat(64), linkedSha256: '2'.repeat(64) },
    cost: { blocks: 1, instructionUnit: 'rig-instruction', worstBlock: 1, worstSteady: 1 },
    pcm: { rate: 48000, channels: 2, samples },
  });
  const save = (name: string, samples: number[], emit: AudioExport[], hash?: string) =>
    writeAudioRender(source(samples, hash), path.join(dir, name), new Set(emit));
  return { dir, save };
}

test('render manifests hold provenance and paired stereo comparisons keep per-channel differences', () => {
  const { dir, save } = fixture();
  const a = save('a', [0.25, 0.5, -0.25, -0.5], ['wav', 'json']);
  const b = save('b', [0.25, 0.25, -0.25, -0.25], ['wav', 'json']);
  expect(a.manifest.summary.fingerprint).toMatchObject({ rate: 48000, channels: 2, frames: 2 });
  expect(a.manifest.wav?.file).toBe('audio.wav');
  const { file, report } = writeAudioComparison(a.files.json!, b.files.json!, path.join(dir, 'ab'));
  expect(JSON.parse(readFileSync(file, 'utf8'))).toMatchObject({
    workload: 'same',
    pcm: {
      mode: 'as-recorded',
      identical: false,
      channels: [
        { channel: 1, changedSamples: 0, maxAbs: 0, rms: 0 },
        { channel: 2, changedSamples: 2, maxAbs: 0.25, rms: 0.25 },
      ],
    },
  });
  expect(
    report.plots.map((plot) =>
      readFileSync(path.join(dir, 'ab', plot))
        .subarray(1, 4)
        .toString(),
    ),
  ).toEqual(['PNG', 'PNG']);
});

test('JSON-only comparison warns about workload mismatches without claiming sample evidence', () => {
  const { dir, save } = fixture();
  const a = save('a', [0.25, -0.25, -0.25, 0.25], ['json']);
  const b = save('b', [0.25, 0.25, -0.25, -0.25], ['json'], 'b');
  expect(a.files.wav).toBeUndefined();
  expect(a.manifest.wav).toBeUndefined();
  expect(a.manifest.summary).toMatchObject({ stereoCorrelation: -1, monoRmsDbfs: null });
  const { report } = writeAudioComparison(a.files.json!, b.files.json!, path.join(dir, 'ab'));
  expect(report).toMatchObject({
    workload: 'different',
    warnings: ['Recorded workload settings differ.'],
    delta: { peakDbfs: 0, rmsDbfs: 0 },
    pcm: { mode: 'summary-only' },
    plots: [],
  });
});

test('a bare WAV can compare with another WAV and mismatched rates are not sample-aligned', () => {
  const { dir, save } = fixture();
  const a = save('a', [0.25, 0.25, -0.25, -0.25], ['wav']);
  const b = path.join(dir, 'b.wav');
  writeFileSync(b, wavBytes([0.25, 0.25, -0.25, -0.25], 44100, 2));
  expect(a.files.json).toBeUndefined();
  expect(writeAudioComparison(a.files.wav!, b, path.join(dir, 'ab')).report).toMatchObject({
    workload: 'unknown',
    pcm: { mode: 'summary-only', reason: expect.stringContaining('No automatic resampling') },
  });
  expect(writeAudioComparison(a.files.wav!, a.files.wav!, path.join(dir, 'aa')).report.pcm).toMatchObject({
    mode: 'as-recorded',
    identical: true,
  });
});

test('a manifest cannot silently use a replaced WAV', () => {
  const { dir, save } = fixture();
  const a = save('a', [0.25, 0.25], ['wav', 'json']);
  writeFileSync(a.files.wav!, wavBytes([0.5, 0.5], 48000, 2));
  expect(() => writeAudioComparison(a.files.json!, a.files.json!, path.join(dir, 'ab'))).toThrow(
    'WAV hash does not match',
  );
});

test('the report discloses the spectrum window even when the level envelope extends past it', () => {
  const samples = Array<number>(BAND_WINDOW_FRAMES + 480).fill(0);
  samples.fill(0.25, BAND_WINDOW_FRAMES);
  const summary = summarizeAudio({ samples, rate: 48000, channels: 1 });
  expect(summary.spectrumFrames).toBeLessThan(summary.fingerprint.frames);
  expect(summary.fingerprint.rms_db.at(-1)).toBeGreaterThan(-20);
  expect(summary.periodicityChecked).toBe(false);
});
