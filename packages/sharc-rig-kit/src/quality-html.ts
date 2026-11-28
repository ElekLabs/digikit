/** Render the canonical quality report as an offline Workbench-themed page. */
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { readAudioComparison } from './audio-artifact.ts';
import type { QualityFormat, QualityReport } from './quality-report.ts';

type Comparison = QualityReport['comparisons'][number];
type AudioComparison = ReturnType<typeof readAudioComparison>;
type Take = AudioComparison['baseline'];

const source = (file: string) => readFileSync(new URL(file, import.meta.url), 'utf8');
const theme = '../../../packages/workbench-core/ui/styles/themes/digikit.css';
const fonts = createRequire(new URL('../../../apps/desktop/package.json', import.meta.url));

function fontFace(family: string, file: string, weight: string): string {
  try {
    const bytes = readFileSync(fonts.resolve(file));
    return `@font-face{font-family:"${family}";font-weight:${weight};font-display:swap;src:url(data:font/woff2;base64,${bytes.toString('base64')}) format("woff2")}`;
  } catch {
    // The Workbench tokens supply system fallbacks when the font packages are absent.
    return '';
  }
}

const escape = (value: string) =>
  value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
const code = (value: unknown) => `<code>${escape(JSON.stringify(value) ?? '')}</code>`;
const linkUrl = (file: string) => './' + file.split(path.sep).map(encodeURIComponent).join('/');
const badge = (text: string, style: 'pass' | 'fail' | 'skipped') =>
  `<span class="status ${style}">${escape(text)}</span>`;

function localLink(file: string, label: string): string {
  if (!file || path.isAbsolute(file) || /^[a-z][a-z\d+.-]*:/i.test(file)) return '';
  return `<a href="${escape(linkUrl(file))}">${escape(label)}</a>`;
}

function portable(file: string): boolean {
  return /^quality-media\/(?:comparison|audio|waveform|left|right|plot)-[a-f\d]{64}\.(?:json|wav|png)$/.test(file);
}

function comparisonEvidence(comparison: Comparison, out: string): AudioComparison | undefined {
  if (portable(comparison.file)) return readAudioComparison(path.join(out, comparison.file));
}

function metric(value: unknown, unit: string, digits = 2): string {
  return typeof value === 'number' && Number.isFinite(value) ? `${value.toFixed(digits)} ${unit}` : '—';
}

function overviewField(name: string, value: string) {
  return `<div class="field"><dt>${name}</dt><dd>${value}</dd></div>`;
}

function diagnostics(heading: string, messages: string[]): string {
  if (!messages.length) return '';
  return `<div class="callout"><h3>${escape(heading)}</h3><ul>${messages.map((text) => `<li>${escape(text)}</li>`).join('')}</ul></div>`;
}

function take(label: string, links: Comparison['baseline'], evidence: Take | undefined): string {
  const summary = evidence?.summary;
  const identity = [evidence?.device, evidence?.tap].filter((x) => typeof x === 'string' && x).join(' · ');
  return `<div class="take">
    <h4>${label}</h4>
    ${identity ? `<p class="muted">${escape(identity)}</p>` : ''}
    ${summary ? `<p class="levels">Peak ${metric(summary.fingerprint?.peak_db, 'dBFS')}<br>RMS ${metric(summary.rmsDbfs, 'dBFS')}</p>` : ''}
    <div class="evidence">${links.wav ? localLink(links.wav, 'WAV') : '<span class="muted">WAV unavailable</span>'}
    ${links.file === links.wav ? '' : localLink(links.file, 'Source artifact')}</div>
  </div>`;
}

function listening(comparison: Comparison, out: string, index: number): string {
  const doc = comparisonEvidence(comparison, out);
  const name = doc?.candidate.case ?? doc?.baseline.case;
  const title = typeof name === 'string' && name ? name : `Comparison ${index + 1}`;
  const warnings = Array.isArray(doc?.warnings) ? doc.warnings.filter((w) => typeof w === 'string') : [];
  const audios = [
    { label: 'A · Baseline', file: comparison.baseline.wav },
    { label: 'B · Candidate', file: comparison.candidate.wav },
  ].filter((item) => item.file && portable(item.file));
  const plotLabels: Record<string, string> = {
    left: 'Left channel',
    right: 'Right channel',
    waveform: 'Mono channel',
    plot: 'Comparison plot',
  };
  return `<article class="card comparison">
    <h3>${escape(title)}</h3>
    ${doc?.delta ? `<p class="levels">Candidate − baseline · Δ RMS ${metric(doc.delta.rmsDbfs, 'dB')} · Δ peak ${metric(doc.delta.peakDbfs, 'dB')}</p>` : ''}
    ${doc?.inputMatch === 'same' ? '<p class="muted">Same recorded input</p>' : ''}
    ${doc?.inputMatch === 'different' ? '<p class="muted">Recorded inputs differ</p>' : ''}
    ${warnings.map((warning) => `<p class="muted">${escape(warning)}</p>`).join('')}
    ${doc?.pcm?.mode === 'summary-only' && typeof doc.pcm.reason === 'string' ? `<p class="muted">${escape(doc.pcm.reason)}</p>` : ''}
    <div class="grid">${take('A · Baseline', comparison.baseline, doc?.baseline)}${take('B · Candidate', comparison.candidate, doc?.candidate)}</div>
    <div data-comparison-player>
      <div class="player">
        <div class="takes" role="group" aria-label="Playback version" hidden>
          ${audios.map(({ label }, i) => `<button type="button" aria-pressed="${i === 0}">${label}</button>`).join('')}
        </div>
        ${audios
          .map(
            ({ label, file }, i) =>
              `<audio controls preload="metadata" aria-label="${label}" src="${escape(linkUrl(file!))}"${i ? ' hidden' : ''}></audio>`,
          )
          .join('')}
      </div>
      <p class="status-text" role="status"></p>
    </div>
    ${
      comparison.plots.length
        ? `<div class="grid">
    ${comparison.plots
      .filter(portable)
      .map((plot) => {
        const label = plotLabels[path.basename(plot).split('-')[0]] ?? 'Comparison plot';
        return `<figure class="plot">
        <figcaption>${label} · waveform, difference and band spectrum</figcaption>
        <a href="${escape(linkUrl(plot))}"><img loading="lazy" src="${escape(linkUrl(plot))}" alt="${label} comparison plot at full size"></a>
      </figure>`;
      })
      .join('')}
    </div>`
        : ''
    }
    <div class="evidence">${localLink(comparison.file, 'Comparison JSON')}</div>
  </article>`;
}

function rigCase(c: QualityReport['files'][number]['cases'][number]): string {
  const checks = c.checks
    .map(
      (check) => `<article class="check">
    <h4>${badge(check.ok ? 'Pass' : 'Fail', check.ok ? 'pass' : 'fail')} ${escape(check.kind)}</h4>
    ${check.message ? `<p>${escape(check.message)}</p>` : ''}
    <dl><dt>Observed</dt><dd>${code(check.observed)}</dd>
    <dt>Expected</dt><dd>${code(check.expected)}</dd>
    ${check.reference ? `<dt>Reference</dt><dd>${code(check.reference)}</dd>` : ''}</dl>
  </article>`,
    )
    .join('');
  const { settings, counts, seededFillCounts, linkedSha256 } = c;
  const style = c.status === 'skip' ? 'skipped' : c.status === 'pass' ? 'pass' : 'fail';
  return `<details class="case"${style === 'fail' ? ' open' : ''}>
    <summary>${badge(c.status, style)} <strong>${escape(c.case)}</strong>
    <span class="muted"> · ${escape(c.device ?? 'No device')} · ${escape(c.tap)} · ${c.checks.length} checks</span></summary>
    <div class="case-body">
      ${c.message ? `<p>${escape(c.message)}</p>` : ''}
      ${checks ? `<div class="checks">${checks}</div>` : ''}
      <details><summary>Settings, counts and build</summary>
        <pre>${escape(JSON.stringify({ settings, counts, seededFillCounts, linkedSha256 }, null, 2))}</pre>
      </details>
    </div>
  </details>`;
}

export function qualityHtml(report: QualityReport, out: string, files: Partial<Record<QualityFormat, string>>): string {
  const cases = report.files.flatMap((file) => file.cases);
  const checks = cases.flatMap((c) => c.checks);
  const title = escape(report.module);
  const showListening = report.comparisons.length > 0 || (report.mediaDiagnostics?.length ?? 0) > 0;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self' file:; media-src 'self' file:; font-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>${title} · DSP quality</title>
<style>
${fontFace('Archivo Variable', '@fontsource-variable/archivo/files/archivo-latin-wdth-normal.woff2', '100 900')}
${fontFace('JetBrains Mono', '@fontsource/jetbrains-mono/files/jetbrains-mono-latin-400-normal.woff2', '400')}
${source(theme)}
${source('./quality-html.css')}
</style>
</head>
<body>
<header><div class="wrap">
  <strong>digikit</strong>
  <nav aria-label="Report sections">
    ${showListening ? '<a href="#listening">Listening</a>' : ''}
    <a href="#rig-cases">Rig cases</a>
    ${files.json ? localLink('quality.json', 'Report JSON') : ''}
    ${files.markdown ? localLink('quality.md', 'Markdown') : ''}
  </nav>
</div></header>
<main class="wrap">
  <section class="hero" aria-labelledby="title">
    <div class="heading"><div>
      <p class="muted">DSP quality</p>
      <h1 id="title">${title}</h1>
    </div>${badge(`Rig checks ${report.outcome}`, report.outcome)}</div>
    <p class="scope">Scope covers module-owned declarative SHARC rig cases. Standalone TypeScript tests, firmware behavior and listening review have separate evidence.</p>
    <dl class="overview">
      ${overviewField('Cases passed', `${cases.filter((c) => c.status === 'pass').length} / ${cases.length}`)}
      ${overviewField('Checks passed', `${checks.filter((check) => check.ok).length} / ${checks.length}`)}
      ${overviewField('Rig sample rate', `${report.sampleRate / 1000} kHz`)}
    </dl>
    ${diagnostics('Rig and report diagnostics', report.diagnostics)}
  </section>
  ${
    showListening
      ? `<section id="listening" aria-labelledby="listening-title">
    <div class="heading"><h2 id="listening-title">A/B listening</h2>${report.comparisons.length ? '<p class="muted">Listening review pending</p>' : ''}</div>
    ${diagnostics('Media diagnostics', report.mediaDiagnostics ?? [])}
    ${report.comparisons.map((comparison, index) => listening(comparison, out, index)).join('')}
  </section>`
      : ''
  }
  <section id="rig-cases" aria-labelledby="rig-cases-title">
    <h2 id="rig-cases-title">Rig cases</h2>
    ${
      report.files.length
        ? report.files
            .map(
              (file) => `<article class="file">
      <h3>${escape(file.file)}</h3>
      ${file.sha256 ? `<p class="muted">Case file SHA-256 <code>${escape(file.sha256)}</code></p>` : ''}
      ${file.message ? `<p>${badge('Fail', 'fail')} ${escape(file.message)}</p>` : ''}
      ${file.cases.map(rigCase).join('')}
    </article>`,
            )
            .join('')
        : '<p class="muted">No rig cases reported.</p>'
    }
  </section>
  <footer>${
    Object.keys(report.provenance).length
      ? `<details><summary>Build tool provenance</summary>
    <pre>${escape(JSON.stringify(report.provenance, null, 2))}</pre>
  </details>`
      : escape(report.schema)
  }</footer>
</main>
<script>
${source('./quality-listen.js')}
</script>
</body>
</html>
`;
}
