// What the Calibration section of the Profile pane shows, as pure functions: the calibration files from their texts, the table of a run (per case and
// DSP row), the capture set files (`dk.calcaptures/1`), the request of the fit and the lines of its result, the progress of a run. The section
// (apps/desktop/ui/src/CalibrationSection.tsx) holds signals and handlers only. Host tests: tests/calibration-view.test.ts.
import {
  addModuleCases,
  parseCalCase,
  parseCalProfile,
  type CalCaseFile,
  type CalProfile,
} from '../calibration/calprofile.ts';
import { parseStepFile, type StepLibrary } from '../calibration/steps.ts';
import type { Capture } from '../calibration/runner.ts';

/** A `dk.profile/1` document as the section reads it (the calibration fields only). */
export interface CalProfileDoc {
  schema?: string;
  run?: { id?: string; case_id?: string; target?: string };
  dsp?: { unit?: string; blocks?: number; max_stock_block_delta?: number };
  dsp_rows?: { row: number; kind?: string; calls: number; max_call: number; max_block: number }[];
}

/** The side of a capture: the emulator counts instructions (`dsp.unit` `insn`), a device cycles. */
export type Side = 'emu' | 'dev';

export const sideOf = (p: CalProfileDoc): Side | undefined => {
  const unit = p.dsp?.unit;
  if (unit === 'insn') return 'emu';
  if (unit === 'cycles') return 'dev';
  const id = p.run?.id ?? '';
  return id.startsWith('emu-') ? 'emu' : id.startsWith('dev-') ? 'dev' : undefined;
};

// ---- the calibration files ----

/** The texts of the files of `devices/calibration/` and the module cases, by path. */
export interface CalFileTexts {
  profiles: Record<string, string>;
  steps: Record<string, string>;
  cases: Record<string, string>;
}

export interface CalFiles {
  /** The profiles by name (`light`, `heavy`, `modules`), module cases added to those that discover them. */
  profiles: CalProfile[];
  steps: StepLibrary;
}

const baseName = (p: string) => (p.split('/').pop() ?? p).replace(/\.toml$/, '');

/** Parse every file; a broken file throws with its path (the pane shows the line). Profiles are ordered by name. */
export function loadCalibrationFiles(t: CalFileTexts): CalFiles {
  const steps: Record<string, ReturnType<typeof parseStepFile>> = {};
  for (const [path, text] of Object.entries(t.steps)) {
    const s = parseStepFile(text, path);
    steps[s.name] = s;
  }
  const found: CalCaseFile[] = Object.entries(t.cases).map(([path, text]) => parseCalCase(text, path));
  const profiles = Object.entries(t.profiles)
    .sort(([a], [b]) => (baseName(a) < baseName(b) ? -1 : 1))
    .map(([path, text]) => addModuleCases(parseCalProfile(text, path), found));
  return { profiles, steps };
}

// ---- the table ----

export interface CalRow {
  key: string;
  /** `bd · row 3 · block`: the case, the DSP row and the statistic. */
  label: string;
  case: string;
  /** `row 3 · block`: the row and the statistic, under the case heading. */
  what: string;
  /** Instructions per block (or call) of the emulator, cycles of the device; undefined: not captured. */
  emu?: number;
  dev?: number;
  /** device / model, and the error of the fit factor on this pair, from the fit result (undefined before a fit or when the pair was refused). */
  factor?: number;
  error?: number;
}

const STATS = ['max_block', 'max_call'] as const;
const STAT_TEXT: Record<(typeof STATS)[number], string> = { max_block: 'block', max_call: 'call' };

/** The worst-case values of a set of captures: per case, row and statistic the largest over the captures (what the fit pairs). */
export function worstValues(captures: readonly CalProfileDoc[]): Map<string, { label: string; value: number }> {
  const out = new Map<string, { label: string; value: number }>();
  for (const c of captures) {
    const id = c.run?.case_id;
    if (!id) continue;
    for (const r of c.dsp_rows ?? []) {
      if (r.calls <= 0 && r.max_block <= 0) continue;
      for (const stat of STATS) {
        const key = `${id}|row ${r.row}|${stat}`;
        const value = r[stat];
        const had = out.get(key);
        if (!had || value > had.value) out.set(key, { label: `${id} · row ${r.row} · ${STAT_TEXT[stat]}`, value });
      }
    }
  }
  return out;
}

/** What the fit result says of one pair; read from `dsp.pairs` of a `dk.calibration/1`. */
export interface FitPairs {
  [key: string]: { factor: number; error: number };
}

export function fitPairs(fit: CalibrationDoc | undefined): FitPairs {
  const out: FitPairs = {};
  for (const p of fit?.dsp?.pairs ?? []) out[`${p.case}|${p.id}|${p.stat}`] = { factor: p.ratio, error: p.error };
  return out;
}

/** The rows of the table: the emulator column from the emulator captures, the device column from the device captures, factor and error from the fit. */
export function tableRows(
  sets: { emu?: readonly CalProfileDoc[]; dev?: readonly CalProfileDoc[] },
  fit?: CalibrationDoc,
): CalRow[] {
  const emu = worstValues(sets.emu ?? []);
  const dev = worstValues(sets.dev ?? []);
  const pairs = fitPairs(fit);
  const keys = [...new Set([...emu.keys(), ...dev.keys()])];
  return keys
    .map((key): CalRow => {
      const [kase, id, stat] = key.split('|') as [string, string, (typeof STATS)[number]];
      const row: CalRow = {
        key,
        label: (emu.get(key) ?? dev.get(key))!.label,
        case: kase,
        what: `${id} · ${STAT_TEXT[stat]}`,
      };
      const e = emu.get(key);
      const d = dev.get(key);
      if (e) row.emu = e.value;
      if (d) row.dev = d.value;
      const p = pairs[key];
      if (p) {
        row.factor = p.factor;
        row.error = p.error;
      }
      return row;
    })
    .sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

/** The rows in runs of one case, for the headings of the table. */
export function groupByCase(rows: readonly CalRow[]): { case: string; rows: CalRow[] }[] {
  const out: { case: string; rows: CalRow[] }[] = [];
  for (const r of rows) {
    const last = out[out.length - 1];
    if (last && last.case === r.case) last.rows.push(r);
    else out.push({ case: r.case, rows: [r] });
  }
  return out;
}

export const fmtCount = (n: number | undefined): string =>
  n === undefined ? '–' : Math.round(n).toLocaleString('en-US');
export const fmtFactor = (n: number | undefined): string => (n === undefined ? '–' : n.toFixed(2));
/** A signed error as a percentage: `+1.8 %`, `-0.4 %`. */
export const fmtError = (e: number | undefined): string =>
  e === undefined ? '–' : `${e > 0 ? '+' : ''}${(e * 100).toFixed(1)} %`;
const pct = (f: number): string => `${(f * 100).toFixed(1)} %`;

// ---- capture sets ----

export const CAPTURES_SCHEMA = 'dk.calcaptures/1';

/** The captures of one run on one side. */
export interface CaptureSet {
  side: Side;
  /** The profile that was played (its name), when known. */
  profile?: string;
  /** Where it came from: file names, or `this run`. */
  source: string;
  captures: CalProfileDoc[];
}

/** The file the pane saves a run in: the `dk.profile/1` of every capture, in order. */
export function captureSetFile(set: { profile: string; captures: readonly Pick<Capture, 'profile'>[] }): {
  text: string;
  name: string;
} {
  const docs = set.captures.map((c) => c.profile);
  const side = docs.length > 0 ? sideOf(docs[0] as CalProfileDoc) : undefined;
  return {
    text: `${JSON.stringify({ schema: CAPTURES_SCHEMA, profile: set.profile, side, captures: docs })}\n`,
    name: `calibration-${set.profile.replace(/[^A-Za-z0-9._-]/g, '_')}-${side ?? 'run'}.captures.json`,
  };
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isNum = (v: unknown): boolean => typeof v === 'number' && Number.isFinite(v);

/** Refuse a capture the table and the fit cannot read: wrong schema, no case, a malformed `dsp` header, `dsp_rows` or `spans`. */
function checkCapture(d: unknown, where: string): void {
  if (!isObj(d) || d.schema !== 'dk.profile/1') throw new Error(`${where}: a capture is not a dk.profile/1 document`);
  const run = d.run;
  if (!isObj(run) || typeof run.case_id !== 'string' || run.case_id === '')
    throw new Error(`${where}: a capture has no run.case_id (a calibration run writes it)`);
  if (d.dsp !== undefined) {
    const h = d.dsp;
    const bad =
      !isObj(h) ||
      (h.unit !== undefined && typeof h.unit !== 'string') ||
      (h.blocks !== undefined && !isNum(h.blocks)) ||
      (h.max_stock_block_delta !== undefined && !isNum(h.max_stock_block_delta));
    if (bad)
      throw new Error(
        `${where}: capture ${run.case_id}: the dsp header is malformed (unit text, blocks and max_stock_block_delta numbers)`,
      );
  }
  if (d.dsp_rows !== undefined) {
    if (!Array.isArray(d.dsp_rows)) throw new Error(`${where}: capture ${run.case_id}: dsp_rows is not a list`);
    for (const r of d.dsp_rows)
      if (!isObj(r) || !['row', 'calls', 'max_call', 'max_block'].every((k) => isNum(r[k])))
        throw new Error(`${where}: capture ${run.case_id}: a dsp row lacks numbers row, calls, max_call and max_block`);
  }
  if (d.spans !== undefined) {
    if (!Array.isArray(d.spans)) throw new Error(`${where}: capture ${run.case_id}: spans is not a list`);
    for (const r of d.spans)
      if (!isObj(r) || !isNum(r.owner) || !isNum(r.calls) || !isNum(r.sum_ticks) || !isNum(r.max_ticks))
        throw new Error(`${where}: capture ${run.case_id}: a span lacks numbers owner, calls, sum_ticks and max_ticks`);
  }
}

/**
 * The set of picked files: one `dk.calcaptures/1` file, or any number of `dk.profile/1` files (the `<case>-<n>.profile.json` of `wb profile calibrate
 * run`). Throws a line naming the file when one is neither, has no `run.case_id`, or the files mix emulator and device captures.
 */
export function readCaptureSet(files: readonly { name: string; text: string }[]): CaptureSet {
  if (files.length === 0) throw new Error('no file picked');
  const docs: CalProfileDoc[] = [];
  let profile: string | undefined;
  for (const f of files) {
    let v: unknown;
    try {
      v = JSON.parse(f.text);
    } catch (e) {
      throw new Error(`${f.name}: not JSON (${e instanceof Error ? e.message : String(e)})`);
    }
    const o = v as { schema?: string; profile?: string; captures?: unknown };
    if (o?.schema === CAPTURES_SCHEMA) {
      if (!Array.isArray(o.captures)) throw new Error(`${f.name}: ${CAPTURES_SCHEMA} without captures`);
      if (typeof o.profile === 'string') profile = o.profile;
      docs.push(...(o.captures as CalProfileDoc[]));
    } else if (o?.schema === 'dk.profile/1') docs.push(o as CalProfileDoc);
    else throw new Error(`${f.name}: neither ${CAPTURES_SCHEMA} nor dk.profile/1`);
  }
  for (const d of docs) checkCapture(d, files.length === 1 ? files[0]!.name : files.map((f) => f.name).join(', '));
  const sides = new Set(docs.map(sideOf));
  if (sides.size !== 1 || sides.has(undefined))
    throw new Error(
      `${files.map((f) => f.name).join(', ')}: the captures are not all from the emulator or all from a device`,
    );
  return {
    side: [...sides][0]!,
    ...(profile ? { profile } : {}),
    source: files.length === 1 ? files[0]!.name : `${files.length} files`,
    captures: docs,
  };
}

// ---- the fit ----

/** The parts of `dk.calibration/1` the section reads. */
export interface CalibrationDoc {
  schema?: string;
  key?: { device?: string; firmware_sha256?: string; device_unit?: string; block_cycles?: number };
  dsp?: SectionDoc;
  cf?: SectionDoc;
  refused?: { section: string; case: string; id: string; reason: string }[];
  warnings?: string[];
}
interface SectionDoc {
  factor: { fit: number; budget: number };
  error: { n: number; mean: number; worst_under: number; loo_worst?: number | null; loo_budget_under?: number | null };
  steady?: { n: number; mean: number; worst_under: number };
  stock?: { avg: number; max: number; cases: number };
  pairs?: { case: string; id: string; stat: string; model: number; device: number; ratio: number; error: number }[];
}

/** `dkprof.calibrateFit`'s request from the emulator set and the device set. */
export function fitRequest(
  emu: CaptureSet,
  dev: CaptureSet,
): { device: unknown[]; model: unknown[]; sources: unknown } {
  if (emu.side !== 'emu' || dev.side !== 'dev') throw new Error('a fit pairs one emulator run with one device run');
  if (emu.profile && dev.profile && emu.profile !== dev.profile)
    throw new Error(
      `the emulator run is of profile ${emu.profile} and the device run of ${dev.profile}: a fit needs the same profile on both`,
    );
  return { device: dev.captures, model: emu.captures, sources: { model: emu.source, device: dev.source } };
}

export interface FitView {
  /** `fit 1.32 · budget 1.34 · worst under 1.8 % · n 9`, or why there is none. */
  headline: string;
  /** The other lines: leave-one-out, steady means, stock, ColdFire. */
  details: string[];
  refused: string[];
  warnings: string[];
  /** The result has a DSP fit (a file worth saving). */
  hasFit: boolean;
}

const sectionLine = (s: SectionDoc) =>
  `fit ${fmtFactor(s.factor.fit)} · budget ${fmtFactor(s.factor.budget)} · worst under ${pct(s.error.worst_under)} · n ${s.error.n}`;

/** The lines of a `dk.calibration/1`: a null leave-one-out is `n/a` (one case or row cannot be left out). */
export function fitView(c: CalibrationDoc): FitView {
  const details: string[] = [];
  const d = c.dsp;
  if (d) {
    const e = d.error;
    details.push(
      e.loo_worst == null
        ? 'leave-one-out: n/a (one row only)'
        : `leave-one-out: worst under ${pct(e.loo_worst)}, budget under ${pct(e.loo_budget_under ?? 0)}`,
    );
    if (d.steady && d.steady.n > 0)
      details.push(
        `steady means: n ${d.steady.n} · mean ${fmtError(d.steady.mean)} · worst under ${pct(d.steady.worst_under)}`,
      );
    if (d.stock)
      details.push(
        `stock block delta: mean ${fmtCount(d.stock.avg)} · max ${fmtCount(d.stock.max)} (${d.stock.cases} case${d.stock.cases === 1 ? '' : 's'})`,
      );
  }
  if (c.cf) details.push(`ColdFire spans: ${sectionLine(c.cf)}`);
  return {
    headline: d ? sectionLine(d) : 'no DSP pairs: the captures hold no DSP rows of both sides',
    details,
    refused: (c.refused ?? []).map((r) => `${r.section} ${r.case} ${r.id}: ${r.reason}`),
    warnings: c.warnings ?? [],
    hasFit: !!d,
  };
}

/** `dk-calibration-<device id>.json` */
export const calibrationFileName = (c: CalibrationDoc): string => `dk-calibration-${c.key?.device ?? 'device'}.json`;

// ---- progress ----

/** What a runner log line says that the pane shows: a capture started, or a case was skipped. */
export type RunEvent =
  | { kind: 'capture'; case: string; n: number; of: number }
  /** Before the first case: the cases this rig cannot play (they are skipped, and take no time). */
  | { kind: 'plan'; skipping: string[] }
  | { kind: 'skip'; case: string; reason: string }
  /** A case whose capture counted no call of any module row: recorded as failed, the run goes on. */
  | { kind: 'fail'; case: string; reason: string }
  /** A case needs other modules: the device restarts. */
  | { kind: 'restart'; case: string }
  /** The wait for a stable boot before a restart (`waited` and `of` in seconds). */
  | { kind: 'stable'; waited: number; of: number }
  /** The case's teardown sets the track's own machine type back. */
  | { kind: 'restore'; case: string }
  | { kind: 'text'; text: string };

/** The log lines of `runCalibration` (`case ID: capture N of M`, `case ID: skipped, REASON`, `case ID: failed, REASON`); every other line is text. */
export function parseRunLine(line: string): RunEvent {
  const cap = /^case (\S+): capture (\d+) of (\d+)$/.exec(line);
  if (cap) return { kind: 'capture', case: cap[1]!, n: Number(cap[2]), of: Number(cap[3]) };
  const restart = /^case (\S+): restarting$/.exec(line);
  if (restart) return { kind: 'restart', case: restart[1]! };
  const restore = /^case (\S+): putting the stock machine back$/.exec(line);
  if (restore) return { kind: 'restore', case: restore[1]! };
  const skip = /^case (\S+): skipped, (.*)$/.exec(line);
  if (skip) return { kind: 'skip', case: skip[1]!, reason: skip[2]! };
  const fail = /^case (\S+): failed, (.*)$/.exec(line);
  if (fail) return { kind: 'fail', case: fail[1]!, reason: fail[2]! };
  return { kind: 'text', text: line };
}

/** A restart (to change the loaded modules) and the 30 s stable wait the runtime needs before the next restart, seconds. */
export const RESTART_S = 40;

const runnable = (profile: CalProfile, skipping?: readonly string[]) =>
  profile.cases.filter((c) => !skipping?.includes(c.id));

/** Whether a case needs a restart: its modules differ from those of the case before it. */
const modulesChange = (before: readonly string[], modules: readonly string[]): boolean =>
  [...modules].sort().join('\n') !== [...before].sort().join('\n');

/**
 * The captures of a run: every case that is not skipped (module-free cases like `stock` and the `rest` cases included) times the repeats.
 * `dropped` maps a case the run stopped early (it failed) to the captures it completed; the rest of its repeats do not count.
 */
export function captureTotal(
  profile: CalProfile,
  skipping?: readonly string[],
  dropped?: ReadonlyMap<string, number>,
): number {
  let total = 0;
  for (const c of runnable(profile, skipping)) total += dropped?.has(c.id) ? dropped.get(c.id)! : profile.repeats;
  return total;
}

/** How long a run takes before it starts: every capture at warm-up plus window, and `RESTART_S` for each case whose modules differ from the case before it (the first is compared with no module). */
export function estimateSeconds(profile: CalProfile, skipping?: readonly string[]): number {
  let before: string[] = [];
  let restarts = 0;
  for (const c of runnable(profile, skipping)) {
    if (modulesChange(before, c.modules)) restarts++;
    before = c.modules;
  }
  return captureTotal(profile, skipping) * (profile.warmup_s + profile.capture_s) + restarts * RESTART_S;
}

/**
 * Seconds left of a run, from the position alone (no elapsed time): the captures left at warm-up plus window, and `RESTART_S` for each later
 * case whose modules differ from the case before it. The captures left are the rest of the current case (`n` finished, one more under way
 * when `capturing`; the part of that capture already gone is ignored) and the repeats of every later case. A dropped case has none left.
 * Without a current case (before the first capture): the static estimate.
 */
export function remainingSeconds(p: {
  profile: CalProfile;
  skipping?: readonly string[];
  caseId?: string;
  n: number;
  capturing: boolean;
  dropped?: ReadonlyMap<string, number>;
}): number {
  const cases = runnable(p.profile, p.skipping);
  const at = p.caseId === undefined ? -1 : cases.findIndex((c) => c.id === p.caseId);
  if (at < 0) return Math.ceil(estimateSeconds(p.profile, p.skipping));
  let left = p.dropped?.has(p.caseId!) ? 0 : Math.max(0, p.profile.repeats - p.n - (p.capturing ? 1 : 0));
  let restarts = 0;
  for (let i = at + 1; i < cases.length; i++) {
    left += p.profile.repeats;
    if (modulesChange(cases[i - 1]!.modules, cases[i]!.modules)) restarts++;
  }
  return Math.ceil(left * (p.profile.warmup_s + p.profile.capture_s) + restarts * RESTART_S);
}

/** `capture 7 / 33`: the captures done so far, one more while a capture is under way. */
export function captureCounter(p: { done: number; total: number; capturing: boolean }): string {
  return `capture ${Math.min(p.total, p.done + (p.capturing ? 1 : 0))} / ${p.total}`;
}

/** `45 s`, or `2 min` (rounded up) from one minute on. */
export const fmtLeft = (s: number): string => (s < 60 ? `${s} s` : `${Math.ceil(s / 60)} min`);

/** `capture 7 / 33 · mem · 2 min left` */
export function progressLine(p: { counter: string; case: string; left: number }): string {
  return `${p.counter} · ${p.case} · ${fmtLeft(p.left)} left`;
}

/** The text of the phases between captures: `restarting for mem`, `waiting for a stable boot 4 s / 30 s`, `putting the stock machine back`. */
export function phaseText(e: Extract<RunEvent, { kind: 'restart' | 'stable' | 'restore' }>): string {
  if (e.kind === 'restart') return `restarting for ${e.case}`;
  if (e.kind === 'stable') return `waiting for a stable boot ${e.waited} s / ${e.of} s`;
  return 'putting the stock machine back';
}

/** The progress line of a phase: the counter, then the phase text (`capture 7 / 33 · restarting for mem`). */
export const phaseLine = (counter: string, e: Extract<RunEvent, { kind: 'restart' | 'stable' | 'restore' }>): string =>
  `${counter} · ${phaseText(e)}`;

/** The note before Run: which tracks a run takes over, and that it leaves them on the stock machine (the run starts from the stock machine, it restores nothing). */
export function trackNote(tracks: readonly number[]): string {
  if (tracks.length === 0) return '';
  const runs: string[] = [];
  for (let i = 0; i < tracks.length;) {
    let j = i;
    while (j + 1 < tracks.length && tracks[j + 1] === tracks[j]! + 1) j++;
    runs.push(j - i >= 2 ? `${tracks[i]} to ${tracks[j]}` : tracks.slice(i, j + 1).join(', '));
    i = j + 1;
  }
  const list = runs.join(', ');
  return `Calibration uses ${tracks.length === 1 ? 'track' : 'tracks'} ${list} and leaves ${tracks.length === 1 ? 'it' : 'them'} on the stock machine.`;
}
