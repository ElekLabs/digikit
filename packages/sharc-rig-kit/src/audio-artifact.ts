/** File-based render and comparison evidence. The WAV is optional and never embedded in JSON. */
import { createHash } from 'node:crypto';
import {
  closeSync,
  constants,
  existsSync,
  fstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  realpathSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { audioChannel, comparePcm, summarizeAudio, type AudioPcm, type AudioSummary } from './audio-summary.ts';
import { plotPng, wavBytes, writePng } from './plot.ts';
import type { RenderedCase } from './render-case.ts';
import { parseWave } from './wave.ts';

const RENDER_SCHEMA = 'dk.audio-render/1';
const COMPARE_SCHEMA = 'dk.audio-compare/1';
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');
const writeJson = (file: string, doc: unknown) => writeFileSync(file, JSON.stringify(doc, null, 2) + '\n');
const evidenceJsonLimit = 16 * 1024 * 1024;

/** Read an evidence file without blocking on special files. */
export function readEvidence(file: string, limit?: number): Buffer {
  const fd = openSync(file, constants.O_RDONLY | constants.O_NONBLOCK);
  try {
    const stat = fstatSync(fd);
    if (!stat.isFile()) throw new Error(`${file}: Expected a regular file.`);
    if (limit === undefined) return readFileSync(fd);
    if (stat.size > limit) throw new Error(`${file}: Evidence exceeds ${limit} bytes.`);
    const data = Buffer.allocUnsafe(stat.size + 1);
    let read = 0;
    while (read < data.length) {
      const n = readSync(fd, data, read, data.length - read, null);
      if (n === 0) break;
      read += n;
    }
    if (read !== stat.size) throw new Error(`${file}: Evidence size changed during the read.`);
    return data.subarray(0, read);
  } finally {
    closeSync(fd);
  }
}

export type AudioExport = 'wav' | 'json';
type RenderIdentity = Omit<RenderedCase, 'pcm' | 'profile'>;
export interface RenderManifest extends RenderIdentity {
  schema: typeof RENDER_SCHEMA;
  summary: AudioSummary;
  wav?: { file: string; sha256: string };
  profile?: { file: string; sha256: string };
}

export function writeAudioRender(
  { pcm, profile, ...identity }: RenderedCase,
  dir: string,
  emit: ReadonlySet<AudioExport>,
) {
  const summary = summarizeAudio(pcm);
  const manifest: RenderManifest = { schema: RENDER_SCHEMA, ...identity, summary };
  const files: Partial<Record<AudioExport | 'profile', string>> = {};
  mkdirSync(dir, { recursive: true });
  if (emit.has('wav')) {
    const wav = wavBytes(pcm.samples, pcm.rate, pcm.channels);
    const file = 'audio.wav';
    files.wav = path.join(dir, file);
    writeFileSync(files.wav, wav);
    manifest.wav = { file, sha256: sha256(wav) };
  }
  if (profile) {
    const data = Buffer.from(JSON.stringify(profile) + '\n');
    const digest = sha256(data);
    const file = `profile-${digest.slice(0, 16)}.json`;
    files.profile = path.join(dir, file);
    writeFileSync(files.profile, data);
    manifest.profile = { file, sha256: digest };
  }
  if (emit.has('json')) {
    files.json = path.join(dir, 'render.json');
    writeJson(files.json, manifest);
  }
  return { files, manifest };
}

interface AudioInput {
  file: string;
  summary: AudioSummary;
  pcm?: AudioPcm;
  render?: RenderManifest;
  wavSha256?: string | null;
}

const number = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n);
const nullableNumber = (n: unknown): n is number | null => n === null || number(n);
const hash = (s: unknown): s is string => typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);
const wellFormed = (file: string): boolean => Buffer.from(file, 'utf8').toString('utf8') === file;

function readManifest(file: string): RenderManifest {
  const d = JSON.parse(readEvidence(file, evidenceJsonLimit).toString('utf8')) as RenderManifest | null;
  const s = d?.summary;
  const f = s?.fingerprint;
  if (
    d?.schema !== RENDER_SCHEMA ||
    typeof d.module !== 'string' ||
    typeof d.device !== 'string' ||
    typeof d.case !== 'string' ||
    typeof d.tap !== 'string' ||
    !hash(d.workload?.sha256) ||
    !hash(d.build?.linkedSha256) ||
    !hash(d.build?.rigSha256) ||
    !number(d.cost?.worstBlock) ||
    !s ||
    !f ||
    f.version !== 2 ||
    !Number.isInteger(f.rate) ||
    f.rate < 100 ||
    ![1, 2].includes(f.channels) ||
    !Number.isSafeInteger(f.frames) ||
    f.frames < 1 ||
    !Number.isSafeInteger(f.onset) ||
    !number(f.peak_db) ||
    !Array.isArray(f.bandDb) ||
    !Array.isArray(f.bandHz) ||
    !f.bandDb.every(number) ||
    !f.bandHz.every(number) ||
    f.bandDb.length !== f.bandHz.length ||
    !nullableNumber(s.rmsDbfs) ||
    !nullableNumber(s.monoRmsDbfs) ||
    !number(s.fullScaleFrames) ||
    !nullableNumber(s.monoDeltaDb) ||
    !nullableNumber(s.stereoCorrelation) ||
    !Number.isSafeInteger(s.spectrumFrames) ||
    s.spectrumFrames < 1 ||
    s.spectrumFrames > f.frames ||
    (d.wav !== undefined && (typeof d.wav?.file !== 'string' || !hash(d.wav?.sha256)))
  ) {
    throw new Error(`${file}: invalid ${RENDER_SCHEMA} manifest.`);
  }
  return d;
}

const isRenderManifest = (file: string): boolean => path.extname(file).toLowerCase() === '.json';

function manifestWavPath(file: string, wav: string): string {
  if (!wav || path.basename(wav) !== wav || !/\.wav$/i.test(wav))
    throw new Error(`${file}: WAV reference must be a filename in the manifest directory.`);
  return path.join(path.dirname(realpathSync(file)), wav);
}

function readInput(file: string): AudioInput {
  if (!isRenderManifest(file)) {
    const bytes = readEvidence(file);
    const pcm = parseWave(bytes);
    return { file, pcm, summary: summarizeAudio(pcm), wavSha256: sha256(bytes) };
  }
  const render = readManifest(file);
  if (!render.wav) return { file, render, summary: render.summary, wavSha256: null };
  // A movable render uses a WAV next to its manifest. Do not follow links outside that folder.
  const wavPath = manifestWavPath(file, render.wav.file);
  const wav = realpathSync(wavPath);
  if (path.dirname(wav) !== path.dirname(wavPath))
    throw new Error(`${file}: WAV reference leaves the manifest directory.`);
  const bytes = readEvidence(wav);
  const wavSha256 = sha256(bytes);
  if (wavSha256 !== render.wav.sha256) throw new Error(`${file}: WAV hash does not match the manifest.`);
  const pcm = parseWave(bytes);
  const { rate, channels, frames } = render.summary.fingerprint;
  if (pcm.rate !== rate || pcm.channels !== channels || pcm.samples.length !== frames * channels)
    throw new Error(`${file}: WAV dimensions do not match the manifest.`);
  return { file, render, pcm, summary: render.summary, wavSha256 };
}

const delta = (a: number | null, b: number | null): number | null => (a === null || b === null ? null : b - a);

export function writeAudioComparison(baseline: string, candidate: string, dir: string) {
  const a = readInput(baseline);
  const b = readInput(candidate);
  const x = a.summary.fingerprint;
  const y = b.summary.fingerprint;
  const workload =
    !a.render || !b.render ? 'unknown' : a.render.workload.sha256 === b.render.workload.sha256 ? 'same' : 'different';
  const inputOf = (s: AudioInput) => s.render?.workload.settings?.inputSha256;
  const inputMatch =
    !hash(inputOf(a)) || !hash(inputOf(b)) ? 'unknown' : inputOf(a) === inputOf(b) ? 'same' : 'different';
  const sameGeometry = x.rate === y.rate && x.channels === y.channels && x.frames === y.frames;
  const reason =
    !a.pcm || !b.pcm
      ? 'One or both inputs were exported without WAV.'
      : !sameGeometry
        ? 'Sample rate, channel count or frame count differs. No automatic resampling or alignment.'
        : null;
  const warnings: string[] = [];
  if (workload !== 'same')
    warnings.push(
      workload === 'different'
        ? 'Recorded workload settings differ.'
        : 'Workload identity cannot be checked for a bare WAV.',
    );
  if (a.render && b.render && a.render.device !== b.render.device) warnings.push('Renders target different devices.');
  if (a.render && b.render && a.render.module !== b.render.module) warnings.push('Renders are from different modules.');
  const channels = reason ? null : comparePcm(a.pcm!, b.pcm!);
  const plots: string[] = [];
  const plotSha256: string[] = [];
  mkdirSync(dir, { recursive: true });
  if (channels) {
    for (const { channel, changedSamples } of channels) {
      const file = x.channels === 1 ? 'waveform.png' : channel === 1 ? 'left.png' : 'right.png';
      const png = plotPng(
        x.channels === 1 ? 'A/B MONO' : channel === 1 ? 'A/B LEFT' : 'A/B RIGHT',
        `${changedSamples} changed samples. No gain or alignment applied.`,
        audioChannel(b.pcm!, channel - 1),
        audioChannel(a.pcm!, channel - 1),
        null,
        x.rate,
      );
      writePng(path.join(dir, file), png);
      plots.push(file);
      plotSha256.push(sha256(png));
    }
  }
  const describe = ({ file, render, summary, wavSha256 }: AudioInput) => ({
    file: path.relative(dir, path.resolve(file)),
    module: render?.module,
    device: render?.device,
    case: render?.case,
    tap: render?.tap,
    workloadSha256: render?.workload.sha256,
    inputSha256: render?.workload.settings?.inputSha256,
    build: render?.build,
    cost: render?.cost,
    profile: render?.profile,
    wavSha256,
    summary,
  });
  const report = {
    schema: COMPARE_SCHEMA,
    baseline: describe(a),
    candidate: describe(b),
    workload,
    inputMatch,
    warnings,
    delta: {
      peakDbfs: y.peak_db - x.peak_db,
      rmsDbfs: delta(a.summary.rmsDbfs, b.summary.rmsDbfs),
      monoRmsDbfs: delta(a.summary.monoRmsDbfs, b.summary.monoRmsDbfs),
      fullScaleFrames: b.summary.fullScaleFrames - a.summary.fullScaleFrames,
      worstRigInstructions: a.render && b.render ? b.render.cost.worstBlock - a.render.cost.worstBlock : null,
    },
    pcm: channels
      ? { mode: 'as-recorded' as const, identical: channels.every((c) => c.changedSamples === 0), channels }
      : { mode: 'summary-only' as const, reason },
    plots,
    plotSha256,
  };
  const file = path.join(dir, 'comparison.json');
  writeJson(file, report);
  return { file, report };
}

type AudioComparison = ReturnType<typeof writeAudioComparison>['report'];

function parseAudioComparison(bytes: Buffer): AudioComparison {
  const doc = JSON.parse(bytes.toString('utf8')) as AudioComparison | null;
  if (
    doc?.schema !== COMPARE_SCHEMA ||
    typeof doc.baseline?.file !== 'string' ||
    typeof doc.candidate?.file !== 'string' ||
    !wellFormed(doc.baseline.file) ||
    !wellFormed(doc.candidate.file) ||
    [doc.baseline.wavSha256, doc.candidate.wavSha256].some(
      (value) => value !== undefined && value !== null && !hash(value),
    ) ||
    !Array.isArray(doc.plots) ||
    doc.plots.some((plot) => typeof plot !== 'string' || !wellFormed(plot)) ||
    (doc.plotSha256 !== undefined &&
      (!Array.isArray(doc.plotSha256) || doc.plotSha256.length !== doc.plots.length || !doc.plotSha256.every(hash)))
  )
    throw new Error(`Invalid ${COMPARE_SCHEMA} comparison.`);
  return doc;
}

export function readAudioComparison(file: string): AudioComparison {
  return parseAudioComparison(readEvidence(file, evidenceJsonLimit));
}

export interface ComparisonMedia {
  save(
    file: string,
    media: { kind: 'json' | 'png' | 'wav'; sha256?: string; sibling?: boolean; bytes?: Buffer },
  ): string | undefined;
  unavailable(message: string): void;
}

function localReference(file: string): string {
  if (
    !file ||
    Array.from(file).some((character) => character.charCodeAt(0) < 32) ||
    /^[a-z][a-z\d+.-]*:/i.test(file) ||
    /^[\\/]{2}/.test(file)
  )
    throw new Error(`Expected a local file reference: ${file}`);
  return file;
}

/** Locate saved evidence, optionally collecting portable copies of its media. */
export function audioComparisonLinks(file: string, out: string, media?: ComparisonMedia) {
  if (!wellFormed(file)) throw new Error('Comparison path contains ill-formed Unicode.');
  const bytes = readEvidence(file, evidenceJsonLimit);
  const doc = parseAudioComparison(bytes);
  const dir = path.dirname(path.resolve(file));
  const link = (target: string) => path.relative(out, path.resolve(dir, target));
  const source = (input: AudioComparison['baseline'], label: string): { file: string; wav?: string } => {
    const relative = input.file;
    const artifact = path.resolve(dir, relative);
    const reference = { file: link(artifact) };
    try {
      if (media) {
        localReference(relative);
        if (input.wavSha256 === null) {
          media.unavailable(
            `${label} had no WAV at comparison time. Render with --emit wav,json and regenerate the comparison.`,
          );
          return reference;
        }
      }
      if (!isRenderManifest(artifact)) {
        const wav = media
          ? media.save(artifact, { kind: 'wav', sha256: input.wavSha256 ?? undefined })
          : link(artifact);
        if (wav && media && !input.wavSha256)
          media.unavailable(`${label} WAV has no recorded hash. Regenerate the comparison to verify its provenance.`);
        return wav ? { ...reference, wav } : reference;
      }
      if (!media && !existsSync(artifact)) return reference;
      const { wav } = readManifest(artifact);
      if (wav) {
        const target = manifestWavPath(artifact, wav.file);
        if (!media) return { ...reference, wav: link(target) };
        if (input.wavSha256 && wav.sha256 !== input.wavSha256)
          throw new Error(
            'Render WAV hash changed since comparison. Restore the original render or regenerate the comparison.',
          );
        const saved = media.save(target, { kind: 'wav', sha256: input.wavSha256 ?? wav.sha256, sibling: true });
        if (saved) {
          if (!input.wavSha256)
            media.unavailable(
              `${label} WAV has no recorded comparison hash. Regenerate the comparison to verify its provenance.`,
            );
          return { ...reference, wav: saved };
        }
      } else {
        media?.unavailable(
          input.wavSha256
            ? `${label} WAV recorded by the comparison is unavailable. Restore the original render or regenerate the comparison.`
            : `${label} has no WAV. Render with --emit wav,json to enable playback.`,
        );
      }
      return reference;
    } catch (error) {
      if (!media) throw error;
      media.unavailable(`${label} WAV: ${error instanceof Error ? error.message : String(error)}`);
      return reference;
    }
  };
  const plots = doc.plots.flatMap((plot, index) => {
    if (!media) return [link(plot)];
    try {
      const saved = media.save(path.resolve(dir, localReference(plot)), {
        kind: 'png',
        sha256: doc.plotSha256?.[index],
      });
      return saved ? [saved] : [];
    } catch (error) {
      media.unavailable(`Plot: ${error instanceof Error ? error.message : String(error)}`);
      return [];
    }
  });
  return {
    file: media?.save(path.resolve(file), { kind: 'json', bytes }) ?? link(path.resolve(file)),
    baseline: source(doc.baseline, 'Baseline'),
    candidate: source(doc.candidate, 'Candidate'),
    plots,
  };
}
