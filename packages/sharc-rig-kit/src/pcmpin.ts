/**
 * PCM pins: a numeric fingerprint (v2) of a signal, compared with tolerances (docs/TESTING.md "PCM pins").
 *
 * One comparer serves the rig pins (`tests/rig/fingerprint/<key>.json`, mono, the machine's own output) and the device pins
 * (`<case>.pcm.json`, interleaved stereo from the PCM tap). A pin stores what the signal is, as plain numbers: onset, peak, the level per
 * 10 ms, 64 band levels and, for a periodic signal, pitch, crest factor and harmonics. `compare` returns the differences in plain words.
 * A SHA-256 gates a pin only when it is `exact`. A pin is never set from a hash: it is written from the numeric report of a capture
 * (`pin-cli.ts`) after the report was approved.
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { parse } from 'smol-toml';
import { FS, fft, hanning, peak, rms, wordsHash } from './analysis.ts';
import { BANDS, bandHz, bandLevels, type Fingerprint } from './fingerprint.ts';
import { plotPng, wavBytes } from './plot.ts';
import { parseWave } from './wave.ts';

export interface Tolerance {
  /** frames the onset may move */
  onset: number;
  /** dB: peak and the level per 10 ms */
  level_db: number;
  /** dB: one band */
  band_db: number;
  pitch_cents: number;
  /** dB: one harmonic re the fundamental */
  harmonic_db: number;
  /** percent of the crest factor */
  crest_pct: number;
  /** a band more than this many dB under the loudest band (of pin or capture) is not compared */
  band_floor_db: number;
  /** a 10 ms window under this level (dBFS) in both signals is not compared */
  rms_floor_db: number;
  /** a harmonic under this level (dB re the fundamental) in both signals is not compared */
  harmonic_floor_db: number;
}

/** Tight: a change that can be heard fails. */
export const DEFAULT_TOLERANCE: Tolerance = {
  onset: 0,
  level_db: 0.1,
  band_db: 0.5,
  pitch_cents: 1,
  harmonic_db: 1,
  crest_pct: 0.5,
  band_floor_db: 60,
  rms_floor_db: -90,
  harmonic_floor_db: -80,
};

export interface Periodic {
  pitch: { hz: number; midi: number };
  /** peak over RMS of the analysed window */
  crest: number;
  /** peak over RMS of the second half of the window: the start-up settling of a note is over (the `sineProblems` rule reads it; a pin does not compare it) */
  crest_settled?: number;
  /** harmonics 2..6, dB re the fundamental */
  harmonics_db: number[];
}

export interface FingerprintV2 {
  version: 2;
  rate: number;
  channels: number;
  /** frames analysed (from the onset when `window_frames` is set) */
  frames: number;
  /** frames from the onset that the fingerprint covers; null: the whole signal */
  window_frames: number | null;
  /** first frame with |x| > 1e-6 in the signal, or -1 */
  onset: number;
  peak_db: number;
  /** two channels only */
  stereo: { identical: boolean; max_diff: number } | null;
  /** RMS of each window of `rms_window` frames (all channels), dBFS */
  rms_window: number;
  rms_db: number[];
  /** the centre frequencies of the bands, Hz, and the power of each band, dB re full scale (of the channel mix) */
  bandHz: number[];
  bandDb: number[];
  periodic: Periodic | null;
  /** rig pins: RMS per block of `block` frames, linear (the classifier of a changed rig output reads it) */
  block?: number;
  block_rms?: number[];
  tolerance?: Partial<Tolerance>;
  /** the pin is exact: the SHA-256 of the analysed float32 words must also match */
  exact?: { sha256: string };
}

export type Pin = Fingerprint | FingerprintV2;

const FLOOR_DB = -200;
const dec = (v: number, d: number): number => Number(v.toFixed(d));
const db20 = (a: number): number => (a > 1e-10 ? Math.max(FLOOR_DB, 20 * Math.log10(a)) : FLOOR_DB);

export interface MeasureOptions {
  rate?: number;
  /** interleaved channels in `x` (default 1) */
  channels?: number;
  /** analyse this many frames from the onset (default: the whole signal) */
  windowFrames?: number | null;
  /** also store the RMS per 32-frame block (rig pins) */
  blockRms?: boolean;
  /** Turn off the whole-window pitch FFT for long program material. */
  periodic?: boolean;
}

export interface Measured {
  fp: FingerprintV2;
  /** the analysed samples, interleaved */
  window: number[];
  /** the channel mix of the window */
  mono: number[];
}

/** Fingerprint v2 of `x` (interleaved channels). */
export function measure(x: ArrayLike<number>, o: MeasureOptions = {}): Measured {
  const rate = o.rate ?? FS;
  const ch = o.channels ?? 1;
  const all = Array.from(x);
  const first = all.findIndex((v) => Math.abs(v) > 1e-6);
  const onset = first < 0 ? -1 : Math.floor(first / ch);
  const windowFrames = o.windowFrames ?? null;
  let window = all;
  if (windowFrames !== null) window = onset < 0 ? [] : all.slice(onset * ch, (onset + windowFrames) * ch);
  const frames = Math.floor(window.length / ch);
  window = window.slice(0, frames * ch);
  const chans = Array.from({ length: ch }, (_, c) => Array.from({ length: frames }, (_, i) => window[i * ch + c]!));
  const mono =
    ch === 1 ? chans[0]! : Array.from({ length: frames }, (_, i) => chans.reduce((s, c) => s + c[i]!, 0) / ch);
  let stereo: FingerprintV2['stereo'] = null;
  if (ch === 2) {
    const maxDiff = chans[0]!.reduce((m, v, i) => Math.max(m, Math.abs(v - chans[1]![i]!)), 0);
    stereo = { identical: maxDiff === 0, max_diff: Number(maxDiff.toPrecision(3)) };
  }
  const win = Math.round(rate / 100);
  const rmsDb: number[] = [];
  for (let f = 0; f + win <= frames; f += win) rmsDb.push(dec(db20(rms(window.slice(f * ch, (f + win) * ch))), 2));
  const fp: FingerprintV2 = {
    version: 2,
    rate,
    channels: ch,
    frames,
    window_frames: windowFrames,
    onset,
    peak_db: dec(db20(peak(window)), 2),
    stereo,
    rms_window: win,
    rms_db: rmsDb,
    bandHz: bandHz(),
    bandDb: bandLevels(mono, rate).map((v) => dec(v, 2)),
    periodic: o.periodic === false ? null : periodicity(mono, rate),
  };
  if (o.blockRms) {
    fp.block = 32;
    fp.block_rms = Array.from({ length: Math.floor(frames / 32) }, (_, b) =>
      Number(rms(window.slice(b * 32 * ch, (b + 1) * 32 * ch)).toPrecision(6)),
    );
  }
  return { fp, window, mono };
}

// ---- pitch, crest, harmonics ----------------------------------------------------------------------------------------------------

const HARMONICS = [2, 3, 4, 5, 6];
const PERIODIC_SHARE = 0.9;

/**
 * Pitch (parabolic peak of the zero-padded Hann spectrum), crest factor and harmonics 2..6 of a periodic signal; null when the
 * fundamental and its first six harmonics hold less than 90 % of the energy (noise, a decay through several tones) or the signal is short.
 */
export function periodicity(x: readonly number[], rate = FS): Periodic | null {
  const n = x.length;
  if (n < 2048) return null;
  let size = 1;
  while (size < 8 * n) size <<= 1;
  const w = hanning(n);
  const re = Array.from<number>({ length: size }).fill(0);
  for (let i = 0; i < n; i++) re[i] = x[i]! * w[i]!;
  const [r, im] = fft(re, Array.from<number>({ length: size }).fill(0));
  const half = size / 2;
  const m2 = Array.from({ length: half }, (_, k) => r[k]! ** 2 + im[k]! ** 2);
  const lo = Math.ceil((20 * size) / rate);
  let kp = lo;
  for (let k = lo; k < half; k++) if (m2[k]! > m2[kp]!) kp = k;
  if (kp <= 1 || kp >= half - 1 || m2[kp]! <= 0) return null;
  const a = Math.log(m2[kp - 1]! + 1e-300) / 2;
  const b = Math.log(m2[kp]!) / 2;
  const c = Math.log(m2[kp + 1]! + 1e-300) / 2;
  const d = (0.5 * (a - c)) / (a - 2 * b + c);
  const kf = kp + (Number.isFinite(d) ? d : 0);
  const hz = (kf * rate) / size;
  const guard = Math.ceil((3 * size) / n);
  const band = (k0: number): [number, number] => [
    Math.max(1, Math.round(k0) - guard),
    Math.min(half - 1, Math.round(k0) + guard),
  ];
  const power = (k0: number): number => {
    const [l, h] = band(k0);
    let s = 0;
    for (let k = l; k <= h; k++) s += m2[k]!;
    return s;
  };
  const top = (k0: number): number => {
    const [l, h] = band(k0);
    let s = 0;
    for (let k = l; k <= h; k++) s = Math.max(s, m2[k]!);
    return s;
  };
  let total = 0;
  for (let k = 1; k < half; k++) total += m2[k]!;
  let tonal = 0;
  for (let h = 1; h <= 6 && h * kf < half - guard; h++) tonal += power(h * kf);
  if (tonal / total < PERIODIC_SHARE) return null;
  const fund = top(kf);
  const harmonics = HARMONICS.map((h) =>
    h * kf < half - guard ? dec(10 * Math.log10(Math.max(top(h * kf), 1e-30) / fund), 1) : FLOOR_DB,
  );
  const r0 = rms(x);
  const tail = x.slice(n >> 1);
  const r1 = rms(tail);
  return {
    pitch: { hz: dec(hz, 3), midi: dec(69 + 12 * Math.log2(hz / 440), 3) },
    crest: dec(r0 > 0 ? peak(x) / r0 : 0, 4),
    crest_settled: dec(r1 > 0 ? peak(tail) / r1 : 0, 4),
    harmonics_db: harmonics,
  };
}

// ---- the old format -------------------------------------------------------------------------------------------------------------

/** A v1 pin as the part of v2 that it holds (no level per 10 ms, no stereo, no pitch: those are not compared). */
export function upgradeV1(fp: Fingerprint): FingerprintV2 {
  return {
    version: 2,
    rate: fp.rate,
    channels: 1,
    frames: fp.samples,
    window_frames: null,
    onset: fp.onset,
    peak_db: dec(db20(fp.peak), 2),
    stereo: null,
    rms_window: 0,
    rms_db: [],
    bandHz: fp.bandHz,
    bandDb: fp.bandDb,
    periodic: null,
    block: fp.block,
    block_rms: fp.rms,
  };
}

/** A pin as the v1 fingerprint that the sample classifier of `fingerprint.ts` reads. */
export function asV1(pin: Pin | null): Fingerprint | null {
  if (pin === null) return null;
  if (pin.version === 1) return pin;
  return {
    version: 1,
    rate: pin.rate,
    samples: pin.frames * pin.channels,
    block: pin.block ?? 32,
    peak: 10 ** (pin.peak_db / 20),
    onset: pin.onset < 0 ? -1 : pin.onset * pin.channels,
    rms: pin.block_rms ?? [],
    bandHz: pin.bandHz,
    bandDb: pin.bandDb,
  };
}

// ---- the comparer ---------------------------------------------------------------------------------------------------------------

const sign = (v: number, d = 1): string => `${v > 0 ? '+' : ''}${v.toFixed(d)}`;

/** Runs `[first, last]` of consecutive indices where `bad(i)` holds. */
function runs(n: number, bad: (i: number) => boolean): [number, number][] {
  const out: [number, number][] = [];
  for (let i = 0; i < n; i++)
    if (bad(i)) {
      const last = out.at(-1);
      if (last && last[1] === i - 1) last[1] = i;
      else out.push([i, i]);
    }
  return out;
}

interface RmsComparison {
  window: number;
  windows: number;
  maxAbsDeltaDb: number | null;
  at: number | null;
}

export interface CompareMeasurements {
  rms?: RmsComparison;
  blockRms?: RmsComparison;
}

export interface CompareOptions {
  /** SHA-256 of the analysed float32 words of the capture (needed to check an exact pin) */
  sha256?: string;
  /**
   * Frames of one device DSP block (`deviceBlockFrames`). With it the onset may move by whole blocks, at most `onsetBlocks`: the key press
   * lands in another block when a change makes each block cheaper (findings 45, 57); the signal from the onset on is the same.
   */
  block?: number;
  /** most blocks the onset may move (default 2) */
  onsetBlocks?: number;
  /** Worst deltas among the RMS windows admitted by the comparison's level floors. */
  measurements?: CompareMeasurements;
}

/** Frames of one DSP block of a device (its `short` id): the sample rate over `dsp_blocks_per_second` of its `devices/*.toml`. */
export function deviceBlockFrames(device: string, rate = FS): number {
  const dir = path.join(import.meta.dirname, '../../../devices');
  for (const name of readdirSync(dir).filter((n) => n.endsWith('.toml'))) {
    const t = parse(readFileSync(path.join(dir, name), 'utf8')) as {
      device?: { short?: string };
      board?: { dsp_blocks_per_second?: number };
    };
    if (t.device?.short === device && t.board?.dsp_blocks_per_second)
      return Math.round(rate / t.board.dsp_blocks_per_second);
  }
  throw new Error(`no device file with a DSP block rate for ${device}`);
}

/**
 * The differences of a capture against a pin, in plain words; empty when the capture is within the tolerances. An exact pin also needs the
 * same SHA-256 (the fingerprint differences are listed all the same).
 */
export function compare(pinIn: Pin, got: FingerprintV2, o: CompareOptions = {}): string[] {
  const pin = pinIn.version === 1 ? upgradeV1(pinIn) : pinIn;
  const tol: Tolerance = { ...DEFAULT_TOLERANCE, ...pin.tolerance };
  const out: string[] = [];
  if (pin.rate !== got.rate || pin.channels !== got.channels) {
    return [`format ${got.rate} Hz x ${got.channels}, the pin has ${pin.rate} Hz x ${pin.channels}`];
  }
  if (pin.frames !== got.frames) out.push(`${got.frames} frames, the pin has ${pin.frames}`);
  const shift = got.onset - pin.onset;
  const blocks = o.block ? Math.abs(shift) / o.block : 0;
  const wholeBlocks = o.block !== undefined && Number.isInteger(blocks) && blocks <= (o.onsetBlocks ?? 2);
  if (Math.abs(shift) > tol.onset && !wholeBlocks)
    out.push(
      `onset moved ${shift} frames (${pin.onset} -> ${got.onset})${o.block ? `; allowed: whole ${o.block}-frame blocks, at most ${o.onsetBlocks ?? 2}` : ''}`,
    );
  if (Math.abs(pin.peak_db - got.peak_db) > tol.level_db)
    out.push(`peak ${sign(got.peak_db - pin.peak_db, 2)} dB (${pin.peak_db} -> ${got.peak_db} dBFS)`);
  if (pin.stereo && got.stereo) {
    if (pin.stereo.identical && !got.stereo.identical)
      out.push(`L and R differ now (max |L-R| ${got.stereo.max_diff}), the pin has L = R`);
    else if (!pin.stereo.identical && got.stereo.identical)
      out.push(`L = R now, the pin has max |L-R| ${pin.stereo.max_diff}`);
    else if (!pin.stereo.identical) {
      const dd = db20(got.stereo.max_diff) - db20(pin.stereo.max_diff);
      if (Math.abs(dd) > tol.level_db)
        out.push(`max |L-R| ${sign(dd)} dB (${pin.stereo.max_diff} -> ${got.stereo.max_diff})`);
    }
  }
  // level per window: the first moved run says where, the others are counted
  const nw = Math.min(pin.rms_db.length, got.rms_db.length);
  const ms = (i: number): number => Math.round((i * pin.rms_window * 1000) / pin.rate);
  const rms: RmsComparison = { window: pin.rms_window, windows: nw, maxAbsDeltaDb: null, at: null };
  if (o.measurements) o.measurements.rms = rms;
  const moved = runs(nw, (i) => {
    const p = pin.rms_db[i]!;
    const g = got.rms_db[i]!;
    const delta = Math.abs(g - p);
    const eligible = Math.max(g, p) > tol.rms_floor_db;
    if (o.measurements && eligible && (rms.maxAbsDeltaDb === null || delta > rms.maxAbsDeltaDb)) {
      rms.maxAbsDeltaDb = delta;
      rms.at = i;
    }
    return delta > tol.level_db && eligible;
  });
  if (moved.length) {
    const [a, b] = moved[0]!;
    let mean = 0;
    for (let i = a; i <= b; i++) mean += got.rms_db[i]! - pin.rms_db[i]!;
    mean /= b - a + 1;
    out.push(
      `level ${sign(mean)} dB from ${ms(a)} ms${b > a ? ` to ${ms(b + 1)} ms` : ''}${moved.length > 1 ? ` (and ${moved.length - 1} more spans)` : ''}`,
    );
  }
  if (pin.rms_db.length === 0 && pin.block_rms && got.block_rms && pin.block === got.block) {
    // a v1 rig pin: the level per block, where it is above 60 dB under the peak
    const lim = Math.max(...pin.block_rms) * 1e-3;
    const windows = Math.min(pin.block_rms.length, got.block_rms.length);
    const blockRms: RmsComparison = {
      window: pin.block ?? 32,
      windows,
      maxAbsDeltaDb: null,
      at: null,
    };
    if (o.measurements) o.measurements.blockRms = blockRms;
    const blk = runs(windows, (i) => {
      const p = pin.block_rms![i]!;
      const g = got.block_rms![i]!;
      const delta = Math.abs(db20(g) - db20(p));
      const eligible = Math.max(p, g) > lim;
      if (o.measurements && eligible && (blockRms.maxAbsDeltaDb === null || delta > blockRms.maxAbsDeltaDb)) {
        blockRms.maxAbsDeltaDb = delta;
        blockRms.at = i;
      }
      return eligible && delta > tol.level_db;
    });
    if (blk.length) {
      const [a, b] = blk[0]!;
      out.push(`level changes in blocks ${a}-${b}${blk.length > 1 ? ` (and ${blk.length - 1} more spans)` : ''}`);
    }
  }
  // bands: only those within `band_floor_db` of the loudest band
  const loud = Math.max(...pin.bandDb, ...got.bandDb) - tol.band_floor_db;
  const delta = got.bandDb.map((v, i) => v - pin.bandDb[i]!);
  const counts = (i: number): boolean => Math.max(got.bandDb[i]!, pin.bandDb[i]!) > loud;
  const bad = runs(Math.min(BANDS, delta.length), (i) => Math.abs(delta[i]!) > tol.band_db && counts(i));
  if (bad.length) {
    const text = bad.slice(0, 3).map(([a, b]) => {
      const seg = delta.slice(a, b + 1);
      const mean = seg.reduce((s, v) => s + v, 0) / seg.length;
      return `${Math.round(pin.bandHz[a]!)}-${Math.round(pin.bandHz[b]!)} Hz ${sign(mean)} dB`;
    });
    out.push(`bands moved: ${text.join('; ')}${bad.length > 3 ? `; ${bad.length - 3} more` : ''}`);
  }
  // pitch, crest, harmonics
  if (pin.periodic) {
    if (!got.periodic) out.push(`no longer periodic (the pin has ${pin.periodic.pitch.hz} Hz)`);
    else {
      const cents = 1200 * Math.log2(got.periodic.pitch.hz / pin.periodic.pitch.hz);
      if (Math.abs(cents) > tol.pitch_cents)
        out.push(`pitch ${sign(cents)} cents (${pin.periodic.pitch.hz} -> ${got.periodic.pitch.hz} Hz)`);
      const crest = (100 * (got.periodic.crest - pin.periodic.crest)) / pin.periodic.crest;
      if (Math.abs(crest) > tol.crest_pct)
        out.push(`crest factor ${sign(crest)} % (${pin.periodic.crest} -> ${got.periodic.crest})`);
      HARMONICS.forEach((h, i) => {
        const p = pin.periodic!.harmonics_db[i]!;
        const g = got.periodic!.harmonics_db[i]!;
        if (Math.max(p, g) <= tol.harmonic_floor_db || Math.abs(g - p) <= tol.harmonic_db) return;
        out.push(`harmonic ${h} ${g > p ? 'rose' : 'fell'} to ${g} dB (the pin has ${p} dB re the fundamental)`);
      });
    }
  } else if (got.periodic && pin.rms_db.length) {
    out.push(`periodic now (${got.periodic.pitch.hz} Hz); the pin is not`);
  }
  if (pin.exact) {
    if (o.sha256 === undefined) out.unshift('exact pin: no SHA-256 of the capture to compare');
    else if (o.sha256 !== pin.exact.sha256)
      out.unshift(
        `exact pin: SHA-256 ${o.sha256} differs from ${pin.exact.sha256}${out.length === 0 ? ' (the fingerprint is equal within tolerance: only bits differ)' : ''}`,
      );
  }
  return out;
}

// ---- the report -----------------------------------------------------------------------------------------------------------------

function lines(fp: FingerprintV2): [string, string][] {
  const lvl = fp.rms_db.length
    ? `${Math.min(...fp.rms_db)} .. ${Math.max(...fp.rms_db)} dBFS (${fp.rms_db.length} x ${Math.round((fp.rms_window * 1000) / fp.rate)} ms)`
    : 'not stored';
  const topBands = fp.bandDb
    .map((v, i) => [v, i] as const)
    .sort((a, b) => b[0] - a[0])
    .slice(0, 3)
    .map(([v, i]) => `${Math.round(fp.bandHz[i]!)} Hz ${v}`)
    .join(', ');
  const rows: [string, string][] = [
    [
      'format',
      `${fp.rate} Hz, ${fp.channels} ch, ${fp.frames} frames${fp.window_frames === null ? '' : ` from the onset`}`,
    ],
    ['onset', `${fp.onset} frames`],
    ['peak', `${fp.peak_db} dBFS`],
    ['stereo', fp.stereo ? (fp.stereo.identical ? 'L = R' : `max |L-R| ${fp.stereo.max_diff}`) : '-'],
    ['level', lvl],
    ['loudest bands', `${topBands} dB`],
  ];
  if (fp.periodic) {
    rows.push(['pitch', `${fp.periodic.pitch.hz} Hz = MIDI ${fp.periodic.pitch.midi}`]);
    rows.push([
      'crest factor',
      fp.periodic.crest_settled === undefined
        ? String(fp.periodic.crest)
        : `${fp.periodic.crest} (second half ${fp.periodic.crest_settled})`,
    ]);
    rows.push(['harmonics 2..6', `${fp.periodic.harmonics_db.join(', ')} dB re the fundamental`]);
  } else rows.push(['periodic', 'no']);
  return rows;
}

/** The full report of a capture, with the pin beside it when there is one: Claude approves a pin from these numbers. */
export function report(got: FingerprintV2, pinIn: Pin | null): string {
  const pin = pinIn === null ? null : pinIn.version === 1 ? upgradeV1(pinIn) : pinIn;
  const a = pin ? lines(pin) : null;
  const b = lines(got);
  const w = Math.max(...b.map(([k]) => k.length));
  return b
    .map(([k, v], i) =>
      a ? `  ${k.padEnd(w)}  expected ${a[i]![1]}\n  ${' '.repeat(w)}  measured ${v}` : `  ${k.padEnd(w)}  ${v}`,
    )
    .join('\n');
}

// ---- files ----------------------------------------------------------------------------------------------------------------------

/** `<dir>/pins/<name>.pcm.json`: the pin of a device case next to its test. */
export const pcmPinFile = (dir: string, name: string): string => path.join(dir, 'pins', `${name}.pcm.json`);

export function readPin(file: string): Pin | null {
  return existsSync(file) ? (JSON.parse(readFileSync(file, 'utf8')) as Pin) : null;
}

/** Numbers of an array stay on one line. */
const stringify = (v: unknown): string =>
  `${JSON.stringify(v, null, 2).replace(/\[([^[\]{}]*)\]/g, (_, body: string) => `[${body.replace(/\s*\n\s*/g, ' ').trim()}]`)}\n`;

export interface PinExtras {
  tolerance?: Partial<Tolerance>;
  /** SHA-256 of the analysed words: the pin is exact */
  exactSha256?: string;
}

/** Writes a pin. The tolerance of the pin that is replaced stays unless `extras.tolerance` gives one. Run after a report was approved. */
export function writePin(file: string, fp: FingerprintV2, extras: PinExtras = {}): void {
  const old = readPin(file);
  const tolerance = extras.tolerance ?? (old?.version === 2 ? old.tolerance : undefined);
  const next: FingerprintV2 = { ...fp, tolerance: undefined, exact: undefined };
  if (tolerance && Object.keys(tolerance).length) next.tolerance = tolerance;
  if (extras.exactSha256) next.exact = { sha256: extras.exactSha256 };
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, stringify(next));
}

// ---- the check of a capture -----------------------------------------------------------------------------------------------------

export interface PinCheckResult {
  ok: boolean;
  differences: string[];
  /** the report to print: measured (and expected) numbers */
  text: string;
  fp: FingerprintV2;
  /** the pin was missing */
  pending: boolean;
}

export interface CheckOptions {
  rate?: number;
  channels?: number;
  /** frames from the onset, used when the pin does not name them (a new pin) */
  windowFrames?: number | null;
  /** where plot.png and the window WAV go on a failure (omit: none written) */
  outDir?: string;
  /** frames of a device DSP block: the onset may move by whole blocks (`CompareOptions`) */
  block?: number;
  onsetBlocks?: number;
  name: string;
}

/**
 * Compares a capture with the pin file. A missing pin is `pending` and not ok; the result's text is the full report. On a failure the plot
 * (`plot.png`, the capture over the pin's band curve) and `<name>.window.wav` are written to `outDir` for listening.
 */
export function checkCapture(pinFile: string, pcm: ArrayLike<number>, o: CheckOptions): PinCheckResult {
  const pin = readPin(pinFile);
  const ch = o.channels ?? 1;
  const windowFrames = pin ? (pin.version === 2 ? pin.window_frames : null) : (o.windowFrames ?? null);
  const m = measure(pcm, {
    rate: o.rate,
    channels: ch,
    windowFrames,
    blockRms: pin !== null && (pin.version === 1 || pin.block_rms !== undefined),
  });
  if (pin === null) {
    writeArtifacts(o, m, null, 'NO PIN');
    return {
      ok: false,
      pending: true,
      differences: [`no pin at ${pinFile}`],
      fp: m.fp,
      text: `measured:\n${report(m.fp, null)}`,
    };
  }
  const differences = compare(pin, m.fp, { sha256: wordsHash(m.window), block: o.block, onsetBlocks: o.onsetBlocks });
  if (differences.length) writeArtifacts(o, m, pin, `PIN DIFFERS  ${differences[0]}`);
  return { ok: differences.length === 0, pending: false, differences, fp: m.fp, text: report(m.fp, pin) };
}

function writeArtifacts(o: CheckOptions, m: Measured, pin: Pin | null, verdict: string): void {
  if (!o.outDir) return;
  mkdirSync(o.outDir, { recursive: true });
  const rate = m.fp.rate;
  writeFileSync(path.join(o.outDir, `${o.name}.window.wav`), wavBytes(m.window, rate, m.fp.channels));
  writeFileSync(
    path.join(o.outDir, `${o.name}.plot.png`),
    plotPng(o.name, verdict, m.mono, null, pin ? pin.bandDb : null),
  );
}

/** The failure message of a test: the differences, the report and the files. */
export function failureMessage(name: string, pinFile: string, r: PinCheckResult, outDir?: string): string {
  const head = r.pending
    ? `${name}: the PCM pin is missing (${pinFile}); measured:`
    : `${name}: the PCM differs from the pin (${pinFile}):\n${r.differences.map((d) => `  - ${d}`).join('\n')}\nexpected vs measured:`;
  const tail = outDir ? `\nplot and WAV: ${outDir}/${name}.plot.png, ${name}.window.wav` : '';
  const how = `\nA pin is written from this report after approval: node packages/sharc-rig-kit/src/pin-cli.ts write --wav CAPTURE.wav --pin ${pinFile}`;
  return `${head}\n${r.text}${tail}${how}`;
}

// ---- float WAV (the capture of the PCM tap) -------------------------------------------------------------------------------------

export function parseFloatWav(d: Buffer): { rate: number; channels: number; samples: number[] } {
  const { format, ...pcm } = parseWave(d);
  if (format !== 'float32') throw new Error('Expected an IEEE float WAV.');
  return pcm;
}

// ---- properties of a test tone --------------------------------------------------------------------------------------------------

export interface SineLimits {
  note: number;
  pitch_cents?: number;
  crest_pct?: number;
  harmonic_db?: number;
  steady_db?: number;
}

/**
 * Properties of a plain sine at MIDI `note` that hold whatever the pin says: pitch within 5 cents of the note, crest factor of the second half of the window (after the start-up settling) within 1 % of
 * sqrt(2), harmonics under -60 dB, the level steady within 0.5 dB. Returns the problems.
 */
export function sineProblems(fp: FingerprintV2, l: SineLimits): string[] {
  const out: string[] = [];
  const p = fp.periodic;
  if (!p) return ['the capture is not periodic'];
  const want = 440 * 2 ** ((l.note - 69) / 12);
  const cents = 1200 * Math.log2(p.pitch.hz / want);
  if (Math.abs(cents) > (l.pitch_cents ?? 5))
    out.push(`pitch ${p.pitch.hz} Hz is ${sign(cents)} cents from the note ${l.note} (${want.toFixed(3)} Hz)`);
  const settled = p.crest_settled ?? p.crest;
  const crest = (100 * (settled - Math.SQRT2)) / Math.SQRT2;
  if (Math.abs(crest) > (l.crest_pct ?? 1)) out.push(`crest factor ${settled} is ${sign(crest)} % from sqrt(2)`);
  const hi = Math.max(...p.harmonics_db);
  if (hi >= (l.harmonic_db ?? -60))
    out.push(`a harmonic is at ${hi} dB re the fundamental (limit ${l.harmonic_db ?? -60})`);
  const spread = Math.max(...fp.rms_db) - Math.min(...fp.rms_db);
  if (spread > (l.steady_db ?? 0.5))
    out.push(`the level moves by ${spread.toFixed(2)} dB (limit ${l.steady_db ?? 0.5})`);
  return out;
}
