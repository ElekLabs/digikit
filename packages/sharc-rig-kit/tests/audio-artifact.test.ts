import { mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, expect, test } from 'vitest';
import {
  audioComparisonLinks,
  writeAudioComparison,
  writeAudioRender,
  type AudioExport,
} from '../src/audio-artifact.ts';
import type { RenderedCase } from '../src/render-case.ts';
import { wavBytes } from '../src/plot.ts';
import { DEVICES } from '../src/spec.ts';
import { summarizeAudio } from '../src/audio-summary.ts';
import { BAND_WINDOW_FRAMES } from '../src/fingerprint.ts';
import { qualityMedia } from '../src/quality-media.ts';

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
    workload: { sha256: hash.repeat(64), seed: 1, settings: { inputSha256: 'c'.repeat(64) } },
    build: { rigSha256: '1'.repeat(64), linkedSha256: '2'.repeat(64) },
    cost: { blocks: 1, instructionUnit: 'rig-instruction', worstBlock: 1, worstSteady: 1 },
    pcm: { rate: 48000, channels: 2, samples },
  });
  const save = (name: string, samples: number[], emit: AudioExport[], hash?: string) =>
    writeAudioRender(source(samples, hash), path.join(dir, name), new Set(emit));
  return { dir, save, source };
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
    inputMatch: 'same',
    warnings: ['Recorded workload settings differ.'],
    delta: { peakDbfs: 0, rmsDbfs: 0 },
    pcm: { mode: 'summary-only' },
    plots: [],
  });
});

test('comparison links locate bare PCM and a symlinked manifest’s WAV', () => {
  const { dir, save } = fixture();
  const samples = [0.25, 0.25, -0.25, -0.25];
  const render = save('render', samples, ['wav', 'json']);
  const alias = path.join(dir, 'linked-render.json');
  symlinkSync(render.files.json!, alias);
  const bare = path.join(dir, 'bare.wave');
  writeFileSync(bare, wavBytes(samples, 48000, 2));
  const { file } = writeAudioComparison(bare, alias, path.join(dir, 'compare'));
  const out = path.join(dir, 'quality');
  const links = audioComparisonLinks(file, out);
  expect(links.baseline).toEqual({ file: '../bare.wave', wav: '../bare.wave' });
  expect(links.candidate.file).toBe('../linked-render.json');
  expect(realpathSync(path.resolve(out, links.candidate.wav!))).toBe(realpathSync(render.files.wav!));
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

test('portable audio omits a WAV replaced after comparison', () => {
  const { dir, save } = fixture();
  const a = save('a', [0.25, 0.25], ['wav', 'json']);
  const b = save('b', [0.5, 0.5], ['wav', 'json']);
  const { file } = writeAudioComparison(a.files.json!, b.files.json!, path.join(dir, 'ab'));
  save('a', [0.75, 0.75], ['wav', 'json']);

  const diagnostics: string[] = [];
  const links = audioComparisonLinks(
    file,
    dir,
    qualityMedia(dir, (message) => diagnostics.push(message)),
  );

  expect(links.baseline).toEqual({ file: 'a/render.json' });
  expect(readFileSync(path.join(dir, links.candidate.wav!))).toEqual(readFileSync(b.files.wav!));
  expect(diagnostics).toEqual([expect.stringContaining('Render WAV hash changed since comparison.')]);
});

test('portable audio omits a WAV added after a summary-only comparison', () => {
  const { dir, save } = fixture();
  const samples = [0.25, 0.25];
  const a = save('a', samples, ['json']);
  const { file } = writeAudioComparison(a.files.json!, a.files.json!, path.join(dir, 'ab'));
  save('a', samples, ['wav', 'json']);

  const diagnostics: string[] = [];
  const links = audioComparisonLinks(
    file,
    dir,
    qualityMedia(dir, (message) => diagnostics.push(message)),
  );

  expect(links.baseline).toEqual({ file: 'a/render.json' });
  expect(links.candidate).toEqual({ file: 'a/render.json' });
  expect(diagnostics).toEqual([
    expect.stringContaining('Baseline had no WAV at comparison time.'),
    expect.stringContaining('Candidate had no WAV at comparison time.'),
  ]);
});

test('reusing a render directory never points unprofiled audio at an older profile', () => {
  const { dir, source } = fixture();
  const emit = new Set<AudioExport>(['json']);
  const rendered = source([0.25, 0.25]);
  const a = writeAudioRender({ ...rendered, profile: { run: { id: 'a' } } }, dir, emit);
  const b = writeAudioRender({ ...rendered, profile: { run: { id: 'b' } } }, dir, emit);
  expect(b.files.profile).not.toBe(a.files.profile);
  expect(JSON.parse(readFileSync(b.files.profile!, 'utf8'))).toEqual({ run: { id: 'b' } });
  const dry = writeAudioRender(rendered, dir, emit);
  expect(JSON.parse(readFileSync(dry.files.json!, 'utf8')).profile).toBeUndefined();
});

test('the report discloses the spectrum window even when the level envelope extends past it', () => {
  const samples = Array<number>(BAND_WINDOW_FRAMES + 480).fill(0);
  samples.fill(0.25, BAND_WINDOW_FRAMES);
  const summary = summarizeAudio({ samples, rate: 48000, channels: 1 });
  expect(summary.spectrumFrames).toBeLessThan(summary.fingerprint.frames);
  expect(summary.fingerprint.rms_db.at(-1)).toBeGreaterThan(-20);
  expect(summary.periodicityChecked).toBe(false);
});
