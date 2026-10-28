// The pure logic of the workbench Log drawer (no DOM, no Tauri): the entries and their cap, the filter, the badge, the copy text, the
// rule that opens the drawer by itself, and the one-line texts of the events the app logs. Host tests: apps/desktop/ui/tests/log.test.ts.
import type { ReloadReport } from '../../../web/src/runtime/reload.ts';
import type { FirmwareRun } from '../../../web/src/firmware/env.ts';
import type { TransferRun } from '../../../web/src/transfer/env.ts';
import type { CardFolder } from './card-core.ts';
import { shortFolder } from './card-core.ts';

export const LOG_SOURCES = ['reload', 'build', 'firmware', 'card', 'transfer', 'emulator', 'reset'] as const;
export type LogSource = (typeof LOG_SOURCES)[number];
export type LogLevel = 'info' | 'error';

export interface LogInput {
  source: LogSource;
  level: LogLevel;
  /** One line. */
  text: string;
  /** Longer output (a compiler log); shown when the entry is expanded. */
  detail?: string;
}
export interface LogEntry extends LogInput {
  id: number;
  /** ms since the epoch. */
  at: number;
}
export interface LogState {
  entries: LogEntry[];
  /** Ids only grow, also after a clear, so a "seen" mark never matches a later entry by mistake. */
  nextId: number;
}

/** The log keeps this many entries; the oldest go first. */
export const LOG_CAP = 500;

export const emptyLog = (): LogState => ({ entries: [], nextId: 1 });

export function addEntry(state: LogState, input: LogInput, at: number): LogState {
  const entry: LogEntry = { ...input, id: state.nextId, at };
  return { entries: [...state.entries, entry].slice(-LOG_CAP), nextId: state.nextId + 1 };
}

export const clearEntries = (state: LogState): LogState => ({ entries: [], nextId: state.nextId });

export interface LogFilter {
  source: LogSource | 'all';
  errorsOnly: boolean;
}

export const filterEntries = (entries: LogEntry[], filter: LogFilter): LogEntry[] =>
  entries.filter(
    (e) => (filter.source === 'all' || e.source === filter.source) && (!filter.errorsOnly || e.level === 'error'),
  );

/** Errors logged after the entry `seenId` (the last one on screen when the drawer was open): the badge of the Log button. */
export const unseenErrors = (entries: LogEntry[], seenId: number): number =>
  entries.filter((e) => e.level === 'error' && e.id > seenId).length;

/** The drawer opens by itself for a failed reload, a failed build, and a guest fault, stall or DSP halt (all `emulator` errors). */
export const autoOpens = (e: Pick<LogInput, 'source' | 'level'>): boolean =>
  e.level === 'error' && (e.source === 'reload' || e.source === 'build' || e.source === 'emulator');

const two = (n: number) => String(n).padStart(2, '0');
/** HH:MM:SS in the local time. */
export function formatTime(at: number): string {
  const d = new Date(at);
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

const TIME_WIDTH = 8;
const SOURCE_WIDTH = 8;
/** One entry as text; a detail follows on indented lines. */
export function entryText(e: LogEntry): string {
  const mark = e.level === 'error' ? ' ✕' : '';
  const head = `${formatTime(e.at)}  ${(e.source + mark).padEnd(SOURCE_WIDTH + 2)}${e.text}`;
  if (!e.detail) return head;
  const indent = ' '.repeat(TIME_WIDTH + 2);
  return `${head}\n${e.detail
    .trimEnd()
    .split('\n')
    .map((l) => indent + l)
    .join('\n')}`;
}

/** The text that Copy puts on the clipboard: the entries given (the visible ones), details included. */
export const copyText = (entries: LogEntry[]): string => entries.map(entryText).join('\n');

// ----- the texts of the events -----

/** "248 ms", "2.3 s". */
export const duration = (ms: number): string => (ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`);

/** A reload that worked. */
export function reloadEntry(report: ReloadReport): LogInput {
  const built = report.build?.built ?? [];
  const parts: string[] = [];
  if (built.length) parts.push(`built ${built.map((b) => b.id).join(', ')} (${duration(report.build?.ms ?? 0)})`);
  const took =
    report.ms !== undefined
      ? ` (${duration(report.ms)})`
      : report.seconds !== undefined
        ? ` (${duration(report.seconds * 1000)})`
        : '';
  switch (report.path) {
    case 'swap':
      parts.push(`swapped ${report.bytes ?? 0} B in ${report.owners?.join(', ') || 'no module'}${took}`);
      break;
    case 'none':
      parts.push(`nothing changed${took}`);
      break;
    case 'dsp':
      parts.push(`DSP only, injected${took}`);
      break;
    default:
      parts.push(`booted the new image, ${report.reason ?? 'the core changed'}${took}`);
  }
  return { source: 'reload', level: 'info', text: parts.join(' · ') };
}

/** A reload that failed: `failure` is `reloadError(e)`. A build error is a `build` entry, the compiler output its detail. */
export function reloadFailureEntry(failure: { status: string; full: string }): LogInput {
  const text = failure.status.replace(/^Reload failed: /, '');
  const build = text.startsWith('build of');
  return {
    source: build ? 'build' : 'reload',
    level: 'error',
    text: build ? text : `failed: ${text}`,
    ...(failure.full ? { detail: failure.full.replace(/^Reload failed: /, '') } : {}),
  };
}

export interface FirmwareLoad {
  /** The file name. */
  name: string;
  device: string;
  version: string;
  /** Whether the load asked for the digikit runtime, and what the controller reported of it. */
  runtime: boolean;
  patch?: { patched: boolean; set?: string; cache_hit?: boolean } | null;
  ms: number;
}

/** A firmware load or boot that worked. */
export function firmwareEntry(f: FirmwareLoad): LogInput {
  let runtime = 'no runtime';
  if (f.patch?.patched)
    runtime = `runtime ${f.patch.set ?? ''}${f.patch.cache_hit ? ' (cached image)' : ' (patched now)'}`.replace(
      '  ',
      ' ',
    );
  else if (f.patch) runtime = 'patched image, loaded as it is';
  else if (f.runtime) runtime = 'runtime requested, not applied';
  return {
    source: 'firmware',
    level: 'info',
    text: `loaded ${f.name} · ${f.device} ${f.version} · ${runtime} · boot ${duration(f.ms)}`,
  };
}

/** One log entry for the Firmware page: a flash, a restore or a saved image (the result in the text, the end message as the detail). */
export const flashEntry = (run: FirmwareRun): LogInput => ({
  source: 'firmware',
  level: run.level,
  text: run.text,
  ...(run.detail ? { detail: run.detail } : {}),
});

export const failureEntry = (source: LogSource, what: string, error: string): LogInput => ({
  source,
  level: 'error',
  text: `${what} failed: ${error.split('\n')[0]}`,
  ...(error.includes('\n') ? { detail: error } : {}),
});

/** A card sync or folder change. */
export function cardEntry(what: string, c: CardFolder): LogInput {
  if (c.error) return { source: 'card', level: 'error', text: `${what}: ${c.error}` };
  const n = c.files.length;
  const where = c.folder ? shortFolder(c.folder) : 'no folder';
  const later = c.reload_needed ? ' · reload to use it' : '';
  return { source: 'card', level: 'info', text: `${what}: ${where} (${n} file${n === 1 ? '' : 's'})${later}` };
}

const PAST: Record<string, string> = {
  Installing: 'installed',
  Restarting: 'restarted',
  Uploading: 'uploaded',
  Deleting: 'deleted',
  Creating: 'created a folder',
};

/** One log entry for a run of the Transfer page: files sent and skipped, the end text; the event lines are the detail. */
export function transferEntry(run: TransferRun): LogInput {
  const detail = run.lines.length ? { detail: run.lines.join('\n') } : {};
  if (run.cancelled)
    return { source: 'transfer', level: 'info', text: `${run.what.toLowerCase()} cancelled`, ...detail };
  if (run.error !== undefined)
    return {
      source: 'transfer',
      level: 'error',
      text: `${run.what.toLowerCase()} failed: ${run.error.split('\n')[0]}`,
      ...detail,
    };
  const sent = run.lines.filter((l) => /^Uploading .* bytes\)$/.test(l)).length;
  const skipped = run.lines.filter((l) => /already on the card/.test(l)).length;
  const counts = [sent ? `sent ${sent}` : '', skipped ? `skipped ${skipped}` : ''].filter(Boolean).join(', ');
  const text = [`${PAST[run.what] ?? run.what.toLowerCase()} on ${run.target}`, counts, run.message ?? '']
    .filter(Boolean)
    .join(' · ');
  return { source: 'transfer', level: 'info', text, ...detail };
}

// ----- emulator health -----

/** What the app last saw of the guest's health. */
export interface Health {
  error: string | null;
  stalled: boolean;
  halted: string | null;
}
export const healthStart = (): Health => ({ error: null, stalled: false, halted: null });

/** A health reading; a field that is left out was not read this time. */
export interface HealthReading {
  error?: string | null;
  stalled?: boolean;
  halted?: string | null;
  /** The stall threshold in emulated ms. */
  stallMs?: number;
}

/** The entries for what changed since `prev`, and the new state. A fault, stall or halt is logged once, when it starts. */
export function healthChanges(prev: Health, now: HealthReading): { health: Health; entries: LogInput[] } {
  const health = { ...prev };
  const entries: LogInput[] = [];
  const error = (text: string) => entries.push({ source: 'emulator', level: 'error', text });
  if (now.error !== undefined) {
    const e = now.error || null;
    if (e && e !== prev.error) error(`fault: ${e}`);
    health.error = e;
  }
  if (now.stalled !== undefined) {
    if (now.stalled && !prev.stalled)
      error(`stall: the main loop stopped${now.stallMs ? ` for more than ${duration(now.stallMs)}` : ''}`);
    if (!now.stalled && prev.stalled) entries.push({ source: 'emulator', level: 'info', text: 'stall ended' });
    health.stalled = now.stalled;
  }
  if (now.halted !== undefined) {
    const h = now.halted || null;
    if (h && h !== prev.halted) error(`DSP halt: ${h}`);
    health.halted = h;
  }
  return { health, entries };
}
