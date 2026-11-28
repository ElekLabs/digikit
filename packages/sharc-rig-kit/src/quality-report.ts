/** Report the evidence from a module's declarative SHARC rig checks. */
import { createHash } from 'node:crypto';
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { FS } from './analysis.ts';
import { audioComparisonLinks } from './audio-artifact.ts';
import { qualityHtml } from './quality-html.ts';
import { qualityMedia } from './quality-media.ts';
import { ROOT, rigBinary } from './rig.ts';
import { runRigFile, type RigCaseEvidence } from './soundcheck.ts';
import { moduleDir } from './spec.ts';
import { patchkitBinary } from './toolchain.ts';

type ComparisonLinks = ReturnType<typeof audioComparisonLinks>;
const extensions = { json: 'json', markdown: 'md', html: 'html' };
export const qualityFormats = Object.keys(extensions);
export type QualityFormat = keyof typeof extensions;
type DefaultFiles = { json: string; markdown: string };
type ReportFiles = Partial<Record<QualityFormat, string>>;

interface RigFileEvidence {
  file: string;
  sha256?: string;
  cases: RigCaseEvidence[];
  message?: string;
}

export interface QualityReport {
  schema: 'dk.module-quality/1';
  module: string;
  scope: 'declarative SHARC rig checks';
  sampleRate: number;
  outcome: 'pass' | 'fail';
  provenance: { rigSha256?: string; patchkitSha256?: string };
  files: RigFileEvidence[];
  comparisons: ComparisonLinks[];
  diagnostics: string[];
  mediaDiagnostics?: string[];
}

const sha256 = (file: string) => createHash('sha256').update(readFileSync(file)).digest('hex');
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function writeQualityReport(
  module: string,
  out: string,
  comparisons?: string[],
): Promise<{ files: DefaultFiles; report: QualityReport }>;
export function writeQualityReport(
  module: string,
  out: string,
  comparisons: string[] | undefined,
  emit: ReadonlySet<QualityFormat>,
): Promise<{ files: ReportFiles; report: QualityReport }>;
export async function writeQualityReport(
  module: string,
  out: string,
  comparisons: string[] = [],
  emit: ReadonlySet<QualityFormat> = new Set(['json', 'markdown']),
) {
  const report: QualityReport = {
    schema: 'dk.module-quality/1',
    module,
    scope: 'declarative SHARC rig checks',
    sampleRate: FS,
    outcome: 'fail',
    provenance: {},
    files: [],
    comparisons: [],
    diagnostics: [],
  };
  let dir: string | undefined;
  let names: string[] = [];
  try {
    dir = path.join(moduleDir(module), 'tests/rig');
    names = readdirSync(dir)
      .filter((name) => name.endsWith('.toml'))
      .sort();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') report.diagnostics.push(message(error));
  }
  if (dir && names.length === 0)
    report.diagnostics.push(`No declarative TOML rig cases found in ${path.relative(ROOT, dir)}.`);

  if (dir && names.length > 0) {
    for (const [field, binary] of [
      ['rigSha256', rigBinary],
      ['patchkitSha256', patchkitBinary],
    ] as const) {
      try {
        report.provenance[field] = sha256(binary());
      } catch (error) {
        report.diagnostics.push(message(error));
      }
    }
    for (const name of names) {
      const file = path.join(dir, name);
      const entry: RigFileEvidence = { file: path.relative(ROOT, file), cases: [] };
      report.files.push(entry);
      try {
        entry.sha256 = sha256(file);
        await runRigFile(module, file, { onCase: (evidence) => entry.cases.push(evidence) });
        if (entry.cases.length === 0) entry.message = 'The file declares no rig cases.';
      } catch (error) {
        entry.message = message(error);
      }
    }
  }
  mkdirSync(out, { recursive: true });
  for (const file of comparisons) {
    try {
      const media = emit.has('html')
        ? qualityMedia(out, (message) => (report.mediaDiagnostics ??= []).push(`Comparison ${file}: ${message}`))
        : undefined;
      report.comparisons.push(audioComparisonLinks(file, out, media));
    } catch (error) {
      (report.mediaDiagnostics ??= []).push(
        `Comparison ${file}: ${message(error).replace(/\.$/, '')}. Restore the saved file or regenerate its comparison.`,
      );
    }
  }
  if (
    report.diagnostics.length === 0 &&
    report.files.length > 0 &&
    report.files.every((file) => !file.message && file.cases.every((c) => c.status === 'pass'))
  )
    report.outcome = 'pass';

  const files: ReportFiles = {};
  for (const format of emit) files[format] = path.join(out, `quality.${extensions[format]}`);
  if (files.json) writeFileSync(files.json, qualityJson(report) + '\n');
  if (files.markdown) writeFileSync(files.markdown, qualityMarkdown(report));
  if (files.html) writeFileSync(files.html, qualityHtml(JSON.parse(qualityJson(report)), out, files));
  return { files, report };
}

// JSON has no non-finite numbers. Keep each distinct from the null used for absent measurements.
function json(value: unknown, space?: number): string {
  return JSON.stringify(
    value,
    (_key, v: unknown) => (typeof v === 'number' && !Number.isFinite(v) ? String(v) : v),
    space,
  );
}

export const qualityJson = (report: QualityReport): string => json(report, 2);

const escape = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
const code = (value: unknown) => `<code>${escape(json(value))}</code>`;
const target = (file: string) =>
  file.split(path.sep).map(encodeURIComponent).join('/').replaceAll('(', '%28').replaceAll(')', '%29');

export function qualityMarkdown(report: QualityReport): string {
  const lines = [
    `# DSP quality · ${escape(report.module)}`,
    '',
    `**${report.outcome.toUpperCase()}** · ${report.scope} · ${report.sampleRate} Hz`,
    '',
    'Scope covers module-owned `tests/rig/*.toml` cases. Standalone TypeScript tests, firmware behavior and listening review are outside this report.',
  ];
  for (const diagnostic of report.diagnostics) lines.push('', `- ${escape(diagnostic)}`);
  if (Object.keys(report.provenance).length) lines.push('', `Build tools ${code(report.provenance)}`);
  for (const file of report.files) {
    lines.push('', `## ${escape(file.file)}`);
    if (file.sha256) lines.push('', `Case file SHA-256 \`${file.sha256}\``);
    if (file.message) lines.push('', `**FAIL** · ${escape(file.message)}`);
    for (const c of file.cases) {
      lines.push(
        '',
        `### ${escape(c.case)} · ${escape(c.device ?? 'no device')} · ${c.tap} · ${c.status.toUpperCase()}`,
        '',
      );
      if (c.message) lines.push(escape(c.message), '');
      lines.push(`Settings ${code(c.settings)}`);
      if (c.linkedSha256) lines.push('', `Linked DSP SHA-256 \`${c.linkedSha256}\``);
      if (c.counts) lines.push('', `Counts ${code(c.counts)}`);
      if (c.seededFillCounts) lines.push('', `Seeded fill counts ${code(c.seededFillCounts)}`);
      if (c.checks.length) lines.push('');
      for (const check of c.checks) {
        lines.push(
          `- **${check.ok ? 'PASS' : 'FAIL'}** · ${escape(check.kind)} · ${escape(check.message || (check.ok ? 'Expectation met.' : 'Expectation failed.'))}`,
          `  Observed ${code(check.observed)}. Expected ${code(check.expected)}.`,
        );
        if (check.reference) lines.push(`  Reference ${code(check.reference)}.`);
      }
    }
  }
  if (report.comparisons.length) {
    lines.push('', '## Listening comparisons', '', 'Listening verdict pending human review.', '');
    for (const comparison of report.comparisons) {
      const baseline = comparison.baseline;
      const candidate = comparison.candidate;
      const wav = (file?: string) => (file ? ` ([WAV](${target(file)}))` : '');
      lines.push(
        `- [Comparison JSON](${target(comparison.file)})`,
        `  · [Baseline](${target(baseline.file)})${wav(baseline.wav)}`,
        `  · [Candidate](${target(candidate.file)})${wav(candidate.wav)}`,
        ...comparison.plots.map((plot) => `  · [${escape(path.basename(plot))}](${target(plot)})`),
      );
    }
  }
  if (report.mediaDiagnostics?.length) {
    lines.push('', '## Media diagnostics');
    for (const diagnostic of report.mediaDiagnostics) lines.push('', `- ${escape(diagnostic)}`);
  }
  return lines.join('\n') + '\n';
}
