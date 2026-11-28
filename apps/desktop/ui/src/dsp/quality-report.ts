import type { FsHost } from '@digi/workbench-core/host/host.ts';
import { sha256Hex } from '@digi/workbench-core/runtime/local-inputs.ts';
import type { EnvelopeFrame, EnvelopeTrace } from '../analyzer/frame';

interface QualityCheck {
  kind: string;
  ok: boolean;
  message?: string;
  observed?: unknown;
  expected?: unknown;
}

interface QualityCase {
  case: string;
  device: string | null;
  tap: string;
  status: 'pass' | 'fail' | 'skip' | 'error';
  message?: string;
  checks: QualityCheck[];
}

interface QualityFile {
  file: string;
  message?: string;
  cases: QualityCase[];
}

interface ComparisonSource {
  file: string;
  wav?: string;
}

export interface QualityComparison {
  file: string;
  baseline: ComparisonSource;
  candidate: ComparisonSource;
  plots: string[];
}

export interface QualityReport {
  module: string;
  scope: string;
  sampleRate: number;
  outcome: 'pass' | 'fail';
  diagnostics: string[];
  mediaDiagnostics?: string[];
  files: QualityFile[];
  comparisons: QualityComparison[];
}

type Reader = Pick<FsHost, 'read'>;
type MediaKind = 'json' | 'wav' | 'png';
const MiB = 1024 * 1024;
const limit = { json: 4 * MiB, wav: 32 * MiB, png: 4 * MiB };
const mediaName = {
  json: /^quality-media\/comparison-[a-f0-9]{64}\.json$/,
  wav: /^quality-media\/audio-[a-f0-9]{64}\.wav$/,
  png: /^quality-media\/(?:left|right|waveform|plot)-[a-f0-9]{64}\.png$/,
};

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

const string = (value: unknown): value is string => typeof value === 'string';
const optionalString = (value: unknown) => value === undefined || string(value);
const listOf = <T>(value: unknown, item: (item: unknown) => item is T): value is T[] =>
  Array.isArray(value) && value.every(item);

function check(value: unknown): value is QualityCheck {
  return record(value) && string(value.kind) && typeof value.ok === 'boolean' && optionalString(value.message);
}

function rigCase(value: unknown): value is QualityCase {
  return (
    record(value) &&
    string(value.case) &&
    (value.device === null || string(value.device)) &&
    string(value.tap) &&
    ['pass', 'fail', 'skip', 'error'].some((status) => value.status === status) &&
    optionalString(value.message) &&
    listOf(value.checks, check)
  );
}

function file(value: unknown): value is QualityFile {
  return record(value) && string(value.file) && optionalString(value.message) && listOf(value.cases, rigCase);
}

function source(value: unknown): value is ComparisonSource {
  return record(value) && string(value.file) && optionalString(value.wav);
}

function comparison(value: unknown): value is QualityComparison {
  return (
    record(value) &&
    string(value.file) &&
    source(value.baseline) &&
    source(value.candidate) &&
    listOf(value.plots, string)
  );
}

const positiveInteger = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value > 0;

function parseJson(bytes: ArrayBuffer, label: string): unknown {
  try {
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  } catch {
    throw new Error(`${label} is not valid UTF-8 JSON.`);
  }
}

export function parseQualityReport(bytes: ArrayBuffer): QualityReport {
  const value = parseJson(bytes, 'quality.json');
  if (!record(value) || value.schema !== 'dk.module-quality/1')
    throw new Error('Expected a dk.module-quality/1 report in quality.json.');
  const { module, scope, sampleRate, outcome, diagnostics, mediaDiagnostics, files, comparisons } = value;
  if (
    !string(module) ||
    !module ||
    !string(scope) ||
    !positiveInteger(sampleRate) ||
    (outcome !== 'pass' && outcome !== 'fail') ||
    !listOf(diagnostics, string) ||
    (mediaDiagnostics !== undefined && !listOf(mediaDiagnostics, string)) ||
    !listOf(files, file) ||
    !listOf(comparisons, comparison)
  )
    throw new Error('quality.json has invalid report fields.');
  return { module, scope, sampleRate, outcome, diagnostics, mediaDiagnostics, files, comparisons };
}

async function readFile(fs: Reader, root: string, name: string, kind: MediaKind): Promise<ArrayBuffer> {
  const bytes = await fs.read(root, name, limit[kind] + 1);
  if (bytes.byteLength > limit[kind]) throw new Error(`${name} exceeds the ${limit[kind] / MiB} MiB preview limit.`);
  return bytes;
}

export async function readQualityReport(fs: Reader, root: string, savedDigest?: string) {
  const bytes = await readFile(fs, root, 'quality.json', 'json');
  const sha256 = await sha256Hex(new Uint8Array(bytes));
  if (savedDigest !== undefined && sha256 !== savedDigest)
    throw new Error('The saved quality.json failed its checksum.');
  return { report: parseQualityReport(bytes), sha256 };
}

export async function readQualityMedia(fs: Reader, root: string, name: string, kind: MediaKind) {
  if (name !== name.trim() || !mediaName[kind].test(name))
    throw new Error('No portable copy is listed under quality-media.');
  const bytes = await readFile(fs, root, name, kind);
  const header = new Uint8Array(bytes, 0, Math.min(12, bytes.byteLength));
  if (
    kind === 'wav' &&
    (bytes.byteLength < 44 ||
      String.fromCharCode(...header.subarray(0, 4)) !== 'RIFF' ||
      String.fromCharCode(...header.subarray(8, 12)) !== 'WAVE')
  )
    throw new Error('Expected RIFF/WAVE audio.');
  if (kind === 'png') {
    const image = new DataView(bytes);
    if (
      bytes.byteLength < 33 ||
      image.getUint32(0) !== 0x89504e47 ||
      image.getUint32(4) !== 0x0d0a1a0a ||
      image.getUint32(8) !== 13 ||
      image.getUint32(12) !== 0x49484452
    )
      throw new Error('Expected a PNG image.');
    const width = image.getUint32(16);
    const height = image.getUint32(20);
    if (!width || !height || width > 4096 || height > 4096)
      throw new Error('PNG dimensions exceed the 4096-pixel preview bounds.');
  }
  const expected = name.slice(name.lastIndexOf('-') + 1, name.lastIndexOf('.'));
  if ((await sha256Hex(new Uint8Array(bytes))) !== expected) throw new Error('The portable copy failed its checksum.');
  return bytes;
}

function envelopeTrace(side: unknown): EnvelopeTrace {
  if (!record(side) || !record(side.summary) || !record(side.summary.fingerprint))
    throw new Error('Comparison has no RMS fingerprint.');
  const { rate, rms_window, rms_db, frames } = side.summary.fingerprint;
  if (
    !positiveInteger(rate) ||
    !positiveInteger(frames) ||
    !positiveInteger(rms_window) ||
    !Array.isArray(rms_db) ||
    rms_db.length !== Math.floor(frames / rms_window) ||
    !rms_db.every((value) => typeof value === 'number' && Number.isFinite(Math.fround(value)))
  )
    throw new Error('Comparison has invalid RMS windows.');
  return {
    levels: Float32Array.from(rms_db),
    secondsPerWindow: rms_window / rate,
    duration: frames / rate,
  };
}

export function parseComparisonEvidence(bytes: ArrayBuffer): { envelope?: EnvelopeFrame; notices: string[] } {
  const value = parseJson(bytes, 'Comparison');
  if (!record(value) || value.schema !== 'dk.audio-compare/1')
    throw new Error('Expected a dk.audio-compare/1 comparison.');
  const notices = Array.isArray(value.warnings) ? value.warnings.filter(string) : [];
  if (value.inputMatch === 'same') notices.unshift('Same recorded input.');
  if (value.inputMatch === 'different') notices.unshift('Recorded inputs differ.');
  try {
    const baseline = envelopeTrace(value.baseline);
    const candidate = envelopeTrace(value.candidate);
    return {
      envelope: { traces: [baseline, candidate], duration: Math.max(baseline.duration, candidate.duration) },
      notices,
    };
  } catch (error) {
    notices.push(`RMS envelope · ${error instanceof Error ? error.message : String(error)}`);
    return { notices };
  }
}
