import { createHash } from 'node:crypto';
import { expect, test, vi } from 'vitest';
import {
  parseComparisonEvidence,
  parseQualityReport,
  readQualityMedia,
  readQualityReport,
} from '../src/dsp/quality-report';

const json = (value: unknown) => new TextEncoder().encode(JSON.stringify(value)).buffer;
const wavPath = `quality-media/audio-${'a'.repeat(64)}.wav`;
const root = '/picked/report';

test('loads failed evidence and preserves the check measurements without following source links', () => {
  const check = { kind: 'level', ok: false, observed: '-Infinity', expected: { min: -80 } };
  const report = {
    schema: 'dk.module-quality/1',
    module: 'some/fx',
    scope: 'declarative SHARC rig checks',
    sampleRate: 48000,
    outcome: 'fail',
    files: [
      {
        file: 'tests/rig/example.toml',
        cases: [{ case: 'tail', device: null, tap: 'fx-scratch', status: 'fail', checks: [check] }],
      },
    ],
    diagnostics: [],
    mediaDiagnostics: ['Missing saved candidate WAV'],
    comparisons: [
      {
        file: '../external.json',
        baseline: { file: '../../dry.wav', wav: wavPath },
        candidate: { file: '../../wet.wav' },
        plots: [],
      },
    ],
  };

  expect(parseQualityReport(json(report)).files[0].cases[0].checks).toEqual([check]);
  expect(() => parseQualityReport(json({ ...report, schema: 'dk.module-quality/2' }))).toThrow('dk.module-quality/1');
  expect(() => parseQualityReport(json({ ...report, files: [{ file: 'case.toml', cases: [null] }] }))).toThrow(
    'invalid report fields',
  );
});

test('rejects external, traversing, spoofed and cross-format references before a host read', async () => {
  const read = vi.fn<() => Promise<ArrayBuffer>>();
  for (const path of [
    '../dry.wav',
    `/tmp/${wavPath}`,
    `https://example.org/${wavPath}`,
    wavPath.replace('/', '/..%2f'),
    wavPath.replace('/', '\\'),
    wavPath + '\n',
    wavPath.replace('.wav', '.png'),
  ]) {
    await expect(readQualityMedia({ read }, root, path, 'wav')).rejects.toThrow('No portable copy');
  }
  expect(read).not.toHaveBeenCalled();
});

test('bounds the initial read and refuses wrongly labeled audio', async () => {
  const wav = new Uint8Array(44);
  wav.set(new TextEncoder().encode('RIFF'));
  wav.set(new TextEncoder().encode('WAVE'), 8);
  const name = `quality-media/audio-${createHash('sha256').update(wav).digest('hex')}.wav`;
  const read = vi.fn(async (_root: string, _path: string, _head: number | null) => wav.buffer);
  await expect(readQualityMedia({ read }, root, name, 'wav')).resolves.toBe(wav.buffer);
  expect(read).toHaveBeenCalledWith(root, name, 32 * 1024 * 1024 + 1);

  read.mockResolvedValueOnce(json({ contents: '<iframe src="https://example.org/"></iframe>' }));
  await expect(readQualityMedia({ read }, root, name, 'wav')).rejects.toThrow('Expected RIFF/WAVE');

  wav[43] = 1;
  await expect(readQualityMedia({ read }, root, name, 'wav')).rejects.toThrow('failed its checksum');

  read.mockResolvedValueOnce(new ArrayBuffer(4 * 1024 * 1024 + 1));
  await expect(readQualityReport({ read }, root)).rejects.toThrow('preview limit');
  expect(read).toHaveBeenLastCalledWith(root, 'quality.json', 4 * 1024 * 1024 + 1);
});

test('uses the recorded window steps and leaves partial trailing windows out of both traces', () => {
  const side = (rate: number, frames: number, window: number, levels: number[]) => ({
    summary: { fingerprint: { rate, frames, rms_window: window, rms_db: levels } },
  });
  const comparison = {
    schema: 'dk.audio-compare/1',
    inputMatch: 'different',
    warnings: ['Renders target different devices.'],
    baseline: side(100, 30, 10, [-100, -20, -5]),
    candidate: side(200, 55, 20, [-76, -12]),
  };
  const { envelope: frame, notices } = parseComparisonEvidence(json(comparison));
  expect(notices).toEqual(['Recorded inputs differ.', 'Renders target different devices.']);
  expect(frame).toBeDefined();
  if (!frame) return;
  expect(frame.duration).toBe(0.3);
  expect(frame.traces[0].secondsPerWindow).toBe(0.1);
  expect(frame.traces[1].duration).toBe(0.275);
  expect(Array.from(frame.traces[1].levels)).toEqual([-76, -12]);
  const summary = parseComparisonEvidence(json({ ...comparison, candidate: side(200, 55, 20, [-76, -12, 0]) }));
  expect(summary.envelope).toBeUndefined();
  expect(summary.notices).toEqual([
    'Recorded inputs differ.',
    'Renders target different devices.',
    'RMS envelope · Comparison has invalid RMS windows.',
  ]);
});
