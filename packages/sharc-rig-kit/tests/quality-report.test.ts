import { existsSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { expect, it } from 'vitest';
import { FS } from '../src/analysis.ts';
import { writeAudioComparison } from '../src/audio-artifact.ts';
import { Raster, wavBytes } from '../src/plot.ts';
import { qualityJson, qualityMarkdown, writeQualityReport, type QualityReport } from '../src/quality-report.ts';

it('renders the same findings and non-finite numbers in JSON and Markdown', () => {
  const location = { case: 'noise range', device: 'fx' };
  const report: QualityReport = {
    schema: 'dk.module-quality/1',
    module: 'test/fx',
    scope: 'declarative SHARC rig checks',
    sampleRate: FS,
    outcome: 'fail',
    provenance: {},
    files: [
      {
        file: 'tests/rig/noise.toml',
        cases: [
          {
            ...location,
            tap: 'fx-scratch',
            settings: { input: { kind: 'noise' }, seed: 15 },
            status: 'fail',
            checks: [
              { ...location, kind: 'rms', ok: true, message: '', observed: { rms: 0.5 }, expected: { max: 0.6 } },
              {
                ...location,
                kind: 'finite',
                ok: false,
                message: 'a sample is not finite',
                observed: { finite: false, firstNonFinite: 5, value: NaN },
                expected: { finite: true },
              },
            ],
          },
        ],
      },
    ],
    comparisons: [],
    diagnostics: [],
  };
  const doc = JSON.parse(qualityJson(report)) as QualityReport;
  expect(doc.files[0]!.cases[0]!.checks.map(({ observed }) => observed)).toEqual([
    { rms: 0.5 },
    { finite: false, firstNonFinite: 5, value: 'NaN' },
  ]);
  const markdown = qualityMarkdown(report);
  expect(markdown).toContain('noise range · fx · fx-scratch · FAIL');
  for (const check of doc.files[0]!.cases[0]!.checks) {
    expect(markdown).toContain(`**${check.ok ? 'PASS' : 'FAIL'}** · ${check.kind}`);
    expect(markdown).toContain(`Observed <code>${JSON.stringify(check.observed)}</code>`);
    expect(markdown).toContain(`Expected <code>${JSON.stringify(check.expected)}</code>`);
  }
});

it('keeps reports for absent rig cases and links a saved A/B without a listening verdict', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'rig-quality-'));
  try {
    const baseline = path.join(dir, 'before.wav');
    const candidate = path.join(dir, 'after.wav');
    writeFileSync(baseline, wavBytes(Array(64).fill(0), FS));
    writeFileSync(candidate, wavBytes(Array(64).fill(0.25), FS));
    const { file } = writeAudioComparison(baseline, candidate, path.join(dir, 'comparison'));
    const { files, report } = await writeQualityReport('test/no-such-module', path.join(dir, 'quality'), [file]);
    expect(report.outcome).toBe('fail');
    expect(report.diagnostics).toContain('unknown module test/no-such-module');
    expect(JSON.parse(readFileSync(files.json, 'utf8')).comparisons).toEqual([
      {
        file: '../comparison/comparison.json',
        baseline: { file: '../before.wav', wav: '../before.wav' },
        candidate: { file: '../after.wav', wav: '../after.wav' },
        plots: ['../comparison/waveform.png'],
      },
    ]);
    const markdown = readFileSync(files.markdown, 'utf8');
    expect(markdown).toContain('Listening verdict pending human review.');
    expect(markdown).toContain('[WAV](../before.wav)');
    expect(markdown).toContain('[waveform.png](../comparison/waveform.png)');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('exports portable comparison media while keeping case and source evidence references', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'rig-quality-'));
  try {
    const baseline = path.join(dir, "be'fore).wav");
    const candidate = path.join(dir, 'after.wav');
    const baselineBytes = wavBytes(Array(64).fill(0), FS);
    writeFileSync(baseline, baselineBytes);
    writeFileSync(candidate, wavBytes(Array(64).fill(0.25), FS));
    const { file } = writeAudioComparison(baseline, candidate, path.join(dir, 'comparison'));
    const comparisonBytes = readFileSync(file);
    const out = path.join(dir, 'quality');
    const { files, report } = await writeQualityReport(
      'test/<script>evil()</script>',
      out,
      [file],
      new Set(['json', 'html']),
    );
    expect(report.outcome).toBe('fail');
    expect(report.mediaDiagnostics).toBeUndefined();
    expect(files.markdown).toBeUndefined();
    expect(existsSync(path.join(out, 'quality.md'))).toBe(false);
    const [comparison] = report.comparisons;
    expect(comparison.file).toMatch(/^quality-media\/comparison-[a-f0-9]{64}\.json$/);
    expect(comparison.baseline.file).toBe("../be'fore).wav");
    expect(qualityMarkdown(report)).toContain("../be'fore%29.wav");
    expect(comparison.baseline.wav).toMatch(/^quality-media\/audio-[a-f0-9]{64}\.wav$/);
    expect(comparison.plots).toEqual([expect.stringMatching(/^quality-media\/waveform-[a-f0-9]{64}\.png$/)]);
    const moved = path.join(dir, 'moved');
    renameSync(out, moved);
    rmSync(baseline);
    expect(readFileSync(path.join(moved, comparison.file))).toEqual(comparisonBytes);
    expect(readFileSync(path.join(moved, comparison.baseline.wav!))).toEqual(baselineBytes);
    expect(readFileSync(path.join(moved, comparison.plots[0]))).toEqual(
      readFileSync(path.join(path.dirname(file), 'waveform.png')),
    );
    const html = readFileSync(path.join(moved, path.basename(files.html!)), 'utf8');
    expect(html).toContain('&lt;script&gt;evil()&lt;/script&gt;');
    expect(html).not.toContain('<script>evil()</script>');
    expect(html).toContain('src="./' + comparison.baseline.wav + '"');
    expect(html).toContain('src="./' + comparison.plots[0] + '"');
    expect(html).toContain('href="./../be&#39;fore).wav"');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('omits damaged or replaced media and retains the comparison document', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'rig-quality-'));
  try {
    const baseline = path.join(dir, 'before.wav');
    const candidate = path.join(dir, 'after.wav');
    const original = wavBytes(Array(64).fill(0), FS);
    writeFileSync(baseline, original);
    writeFileSync(candidate, original);
    const { file } = writeAudioComparison(baseline, candidate, path.join(dir, 'comparison'));
    const plot = path.join(path.dirname(file), 'waveform.png');
    writeFileSync(plot, readFileSync(plot).subarray(0, 12));
    writeFileSync(baseline, original.subarray(0, 12));
    writeFileSync(candidate, wavBytes(Array(64).fill(0.5), FS));

    const { report } = await writeQualityReport(
      'test/no-such-module',
      path.join(dir, 'quality'),
      [file],
      new Set(['html']),
    );
    expect(report.mediaDiagnostics).toEqual([
      expect.stringContaining('PNG is missing complete image data'),
      expect.stringContaining('Truncated WAV'),
      expect.stringContaining('WAV hash does not match the saved evidence'),
    ]);
    expect(report.comparisons).toEqual([
      {
        file: expect.stringContaining('quality-media/comparison-'),
        baseline: { file: '../before.wav' },
        candidate: { file: '../after.wav' },
        plots: [],
      },
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('retains available evidence when optional comparison media is missing', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'rig-quality-'));
  try {
    const baseline = path.join(dir, 'before.wav');
    const candidate = path.join(dir, 'after.wav');
    writeFileSync(baseline, wavBytes(Array(64).fill(0), FS));
    writeFileSync(candidate, wavBytes(Array(64).fill(0.25), FS));
    const { file } = writeAudioComparison(baseline, candidate, path.join(dir, 'comparison'));
    rmSync(candidate);
    rmSync(path.join(path.dirname(file), 'waveform.png'));
    const { files, report } = await writeQualityReport(
      'test/no-such-module',
      path.join(dir, 'quality'),
      [file],
      new Set(['html']),
    );
    expect(report.diagnostics).toEqual(['unknown module test/no-such-module']);
    expect(report.mediaDiagnostics).toEqual([
      expect.stringContaining('Restore the saved file or regenerate its comparison.'),
      expect.stringContaining('Restore the saved file or regenerate its comparison.'),
    ]);
    expect(report.comparisons[0]).toMatchObject({ candidate: { file: '../after.wav' }, plots: [] });
    expect(report.comparisons[0].candidate.wav).toBeUndefined();
    expect(readFileSync(files.html!, 'utf8')).toContain('Media diagnostics');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('renders unreadable comparisons as media diagnostics with no comparisons attached', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'rig-quality-'));
  try {
    const absent = path.join(dir, 'absent.json');
    const invalid = path.join(dir, 'invalid.json');
    writeFileSync(invalid, '{}');
    const { files, report } = await writeQualityReport(
      'test/no-such-module',
      dir,
      [absent, invalid],
      new Set(['json', 'markdown', 'html']),
    );
    expect(report.diagnostics).toEqual(['unknown module test/no-such-module']);
    expect(report.comparisons).toEqual([]);
    expect(report.mediaDiagnostics).toEqual([
      expect.stringContaining('absent.json'),
      expect.stringContaining('Invalid dk.audio-compare/1 comparison.'),
    ]);
    for (const file of Object.values(files)) {
      expect(readFileSync(file, 'utf8')).toContain('absent.json');
      expect(readFileSync(file, 'utf8')).toContain('Invalid dk.audio-compare/1 comparison.');
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

it('omits a valid PNG replaced after comparison and accepts legacy comparisons', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'rig-quality-'));
  try {
    const wav = path.join(dir, 'audio.wav');
    writeFileSync(wav, wavBytes(Array(64).fill(0), FS));
    const { file, report: comparison } = writeAudioComparison(wav, wav, path.join(dir, 'comparison'));
    writeFileSync(path.join(path.dirname(file), comparison.plots[0]), new Raster(1, 1).png());
    const { report } = await writeQualityReport('test/no-such-module', dir, [file], new Set(['html']));
    expect(report.comparisons[0].plots).toEqual([]);
    expect(report.mediaDiagnostics).toEqual([expect.stringContaining('PNG hash does not match the saved evidence.')]);

    const legacy = JSON.parse(readFileSync(file, 'utf8'));
    delete legacy.plotSha256;
    writeFileSync(file, JSON.stringify(legacy));
    const result = await writeQualityReport('test/no-such-module', dir, [file], new Set(['html']));
    expect(result.report.comparisons[0].plots).toHaveLength(1);
    expect(result.report.mediaDiagnostics).toBeUndefined();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
