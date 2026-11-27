// Calibration steps (`dk.steps/1`): an ordered list of actions with named arguments, resolved for one device into concrete operations.
// Pure: no I/O, no clock, no device. The runner (runner.ts) plays the operations; docs/INSTRUMENTATION.md "Calibration runs" describes the format.
import { parse as parseToml } from 'smol-toml';
import type { MidiMap } from '@digi/devices';
import type { ExpectedEffectRoute } from './effects.ts';

export const STEPS_SCHEMA = 'dk.steps/1';

/** A value a caller gives to a step argument. */
export type ArgValue = string | number | boolean;
export type Args = Record<string, ArgValue>;

/** One step as written: an `action` and its fields (strings may hold `{arg}`). */
export interface StepDef {
  action: string;
  [field: string]: unknown;
}

export interface StepFile {
  name: string;
  description: string;
  /** Defaults of the arguments the steps use; a caller's value replaces one. */
  args: Args;
  steps: StepDef[];
}

/** What a profile gives for a step list: a named step file with arguments, or an inline list in the dk:dev `--keys` syntax. */
export type StepRef = { steps: string; args?: Args } | { inline: string };

/** A concrete operation: names resolved to numbers for one device, tracks expanded one per operation. */
export type Op =
  /** `retrigger_ms`: the runner presses the key again every that many ms of every wait of a capture (a voice that ends by itself keeps sounding for the window). */
  | { op: 'midi.note'; ch: number; key: number; velocity: number; retrigger_ms?: number }
  | { op: 'midi.note_off'; ch: number; key: number }
  | { op: 'midi.cc'; ch: number; cc: number; value: number }
  | { op: 'midi.nrpn'; ch: number; msb: number; lsb: number; value14: number }
  | { op: 'midi.start' }
  | { op: 'midi.stop' }
  | { op: 'key'; key: string | number; hold_ms?: number }
  | { op: 'turn'; encoder: string | number; by: number }
  | { op: 'setup'; what: 'machine' | 'filter'; track: number; type: number | string }
  | { op: 'setup'; what: 'effect'; track: 0; type: string; route: ExpectedEffectRoute }
  | {
      op: 'wait';
      ms?: number;
      event?: string;
      match?: Record<string, string | number>;
      within_ms?: number;
      drained?: boolean;
    }
  | { op: 'shot'; name: string }
  | { op: 'capture'; mark: 'start' | 'stop' };

const ACTIONS = new Set([
  'midi.note',
  'midi.note_off',
  'midi.cc',
  'midi.nrpn',
  'midi.start',
  'midi.stop',
  'key',
  'turn',
  'setup',
  'wait',
  'shot',
  'capture',
]);

const isTable = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

function argsOf(file: string, v: unknown): Args {
  if (v === undefined) return {};
  if (!isTable(v)) throw new Error(`${file}: args must be a table`);
  const out: Args = {};
  for (const [k, x] of Object.entries(v)) {
    if (typeof x !== 'string' && typeof x !== 'number' && typeof x !== 'boolean')
      throw new Error(`${file}: argument ${k} must be a string, number or boolean`);
    out[k] = x;
  }
  return out;
}

/** A `dk.steps/1` TOML file. `file` names it in error messages. */
export function parseStepFile(text: string, file: string): StepFile {
  let doc: Record<string, unknown>;
  try {
    doc = parseToml(text) as Record<string, unknown>;
  } catch (e) {
    throw new Error(`${file}: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (doc.schema !== STEPS_SCHEMA) throw new Error(`${file}: schema must be "${STEPS_SCHEMA}"`);
  for (const k of Object.keys(doc))
    if (!['schema', 'name', 'description', 'args', 'step'].includes(k)) throw new Error(`${file}: unknown key ${k}`);
  if (typeof doc.name !== 'string' || doc.name === '') throw new Error(`${file}: name is missing`);
  const raw = doc.step;
  if (!Array.isArray(raw) || raw.length === 0) throw new Error(`${file}: it has no [[step]]`);
  const steps = raw.map((s, i) => {
    if (!isTable(s) || typeof s.action !== 'string') throw new Error(`${file}: step ${i + 1} has no action`);
    if (!ACTIONS.has(s.action)) throw new Error(`${file}: step ${i + 1}: unknown action ${s.action}`);
    return s as StepDef;
  });
  return {
    name: doc.name,
    description: typeof doc.description === 'string' ? doc.description : '',
    args: argsOf(file, doc.args),
    steps,
  };
}

/**
 * The inline syntax of `dk:dev --keys` (tools/patchkit/pk_devreload.py `parse_keys`): comma or newline separated `tap:KEY`, `wait:MS`,
 * `turn:ENC[:BY]` (BY defaults to 1), `shot:NAME`, each with an optional `*N` repeat and `#` comments. `wait:` also takes an event kind
 * (`wait:MOD_LOAD`). A JSON list of steps is not read here (the Python harness keeps it).
 */
export function parseInlineSteps(text: string): StepDef[] {
  const out: StepDef[] = [];
  for (const raw of text.split(/[,\n]/)) {
    const token0 = (raw.split('#')[0] ?? '').trim();
    if (!token0) continue;
    const star = token0.indexOf('*');
    const token = star < 0 ? token0 : token0.slice(0, star);
    const repeat = star < 0 ? 1 : Number(token0.slice(star + 1).trim() || 1);
    const colon = token.indexOf(':');
    const kind = (colon < 0 ? token : token.slice(0, colon)).trim();
    const arg = colon < 0 ? '' : token.slice(colon + 1).trim();
    let step: StepDef;
    if (kind === 'tap' && arg) step = { action: 'key', key: arg };
    else if (kind === 'wait' && /^\d+$/.test(arg)) step = { action: 'wait', ms: Number(arg) };
    else if (kind === 'wait' && /^[A-Z][A-Z0-9_]*$/.test(arg)) step = { action: 'wait', event: arg };
    else if (kind === 'turn' && arg) {
      const at = arg.lastIndexOf(':');
      step =
        at < 0
          ? { action: 'turn', encoder: arg, by: 1 }
          : { action: 'turn', encoder: arg.slice(0, at), by: Number(arg.slice(at + 1)) };
      if (!Number.isInteger(step.by)) throw new Error(`key script: cannot read ${token0} (turn:ENC:BY)`);
    } else if (kind === 'shot' && arg) step = { action: 'shot', name: arg };
    else throw new Error(`key script: cannot read ${token0} (tap:KEY wait:MS turn:ENC:BY shot:NAME)`);
    if (!Number.isInteger(repeat) || repeat < 1) throw new Error(`key script: bad repeat in ${token0}`);
    for (let i = 0; i < repeat; i++) out.push({ ...step });
  }
  return out;
}

/** Replace `{name}` in the strings of `v`. A string that is exactly one `{name}` takes the argument as it is (a number stays a number). */
export function substitute(v: unknown, args: Args, where: string): unknown {
  if (typeof v === 'string') {
    const whole = /^\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(v);
    const get = (name: string): ArgValue => {
      const x = args[name];
      if (x === undefined) throw new Error(`${where}: argument {${name}} has no value`);
      return x;
    };
    if (whole) return get(whole[1]!);
    return v.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_, n: string) => String(get(n)));
  }
  if (Array.isArray(v)) return v.map((x) => substitute(x, args, where));
  if (isTable(v)) return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, substitute(x, args, where)]));
  return v;
}

/** Tracks as written: `3`, `1-16`, `1,3,5-8` or a list of numbers. */
export function parseTracks(v: unknown, where: string): number[] {
  const parts = Array.isArray(v) ? v.map(String) : String(v).split(',');
  const out: number[] = [];
  for (const p of parts) {
    const m = /^\s*(\d+)(?:\s*-\s*(\d+))?\s*$/.exec(p);
    if (!m) throw new Error(`${where}: bad track list ${JSON.stringify(v)}`);
    const a = Number(m[1]);
    const b = m[2] === undefined ? a : Number(m[2]);
    if (a < 1 || b < a || b > 16) throw new Error(`${where}: tracks are 1 to 16 (${JSON.stringify(v)})`);
    for (let t = a; t <= b; t++) out.push(t);
  }
  return out;
}

const int = (v: unknown, name: string, lo: number, hi: number, where: string): number => {
  const n = typeof v === 'string' && v.trim() !== '' ? Number(v) : v;
  if (typeof n !== 'number' || !Number.isInteger(n) || n < lo || n > hi)
    throw new Error(`${where}: ${name} must be an integer ${lo} to ${hi} (got ${JSON.stringify(v)})`);
  return n;
};

const FIELDS: Record<string, string[]> = {
  'midi.note': ['track', 'key', 'velocity', 'retrigger_ms'],
  'midi.note_off': ['track', 'key'],
  'midi.cc': ['track', 'param', 'cc', 'value'],
  'midi.nrpn': ['track', 'param', 'nrpn', 'value', 'value14'],
  'midi.start': [],
  'midi.stop': [],
  key: ['key', 'hold_ms'],
  turn: ['encoder', 'by'],
  setup: ['machine', 'filter', 'effect', 'track', 'route'],
  wait: ['ms', 'event', 'match', 'within_ms', 'drained'],
  shot: ['name'],
  capture: ['mark'],
};

/** The device a step list resolves for: its `[midi]` table (a device without one cannot take a MIDI step). */
export interface ResolveContext {
  midi: MidiMap | undefined;
  /** Names the step list in error messages. */
  where: string;
}

function channelsOf(s: Record<string, unknown>, ctx: ResolveContext, where: string): number[] {
  if (s.track === undefined) throw new Error(`${where}: track is missing`);
  if (!ctx.midi) throw new Error(`${where}: the device has no [midi] table`);
  // "all": every track the device lists a channel for (the audio tracks)
  const tracks = s.track === 'all' ? ctx.midi.trackChannels.map((_, k) => k + 1) : parseTracks(s.track, where);
  return tracks.map((t) => {
    const ch = ctx.midi!.trackChannels[t - 1];
    if (ch === undefined)
      throw new Error(
        `${where}: no factory MIDI channel for track ${t} on this device (set the channel and use a step with a channel)`,
      );
    return ch;
  });
}

function paramOf(s: Record<string, unknown>, ctx: ResolveContext, where: string) {
  if (typeof s.param !== 'string') throw new Error(`${where}: param is missing`);
  const p = ctx.midi?.params[s.param];
  if (!p)
    throw new Error(
      `${where}: this device has no MIDI parameter ${s.param} (known: ${Object.keys(ctx.midi?.params ?? {}).join(', ')})`,
    );
  return p;
}

/** A 7-bit step value on the full 14-bit NRPN range: 0 is 0 and 127 is 16383 (rounded). */
export const nrpnValue = (v7: number): number => Math.round((v7 * 16383) / 127);

/** One substituted step as operations (a track list gives one operation per track). */
function resolveOne(s: Record<string, unknown> & { action: string }, ctx: ResolveContext, where: string): Op[] {
  const allowed = FIELDS[s.action]!;
  for (const k of Object.keys(s))
    if (k !== 'action' && !allowed.includes(k)) throw new Error(`${where}: unknown field ${k}`);
  switch (s.action) {
    case 'midi.note':
      return channelsOf(s, ctx, where).map((ch) => ({
        op: 'midi.note' as const,
        ch,
        key: int(s.key, 'key', 0, 127, where),
        velocity: int(s.velocity ?? 100, 'velocity', 1, 127, where),
        // 0: not retriggered
        ...(int(s.retrigger_ms ?? 0, 'retrigger_ms', 0, 60_000, where) > 0
          ? { retrigger_ms: int(s.retrigger_ms, 'retrigger_ms', 1, 60_000, where) }
          : {}),
      }));
    case 'midi.note_off':
      return channelsOf(s, ctx, where).map((ch) => ({
        op: 'midi.note_off' as const,
        ch,
        key: int(s.key, 'key', 0, 127, where),
      }));
    case 'midi.cc': {
      const cc = s.param !== undefined ? paramOf(s, ctx, where).cc : s.cc;
      if (cc === undefined) throw new Error(`${where}: ${s.param} has no CC on this device`);
      const value = int(s.value, 'value', 0, 127, where);
      return channelsOf(s, ctx, where).map((ch) => ({
        op: 'midi.cc' as const,
        ch,
        cc: int(cc, 'cc', 0, 127, where),
        value,
      }));
    }
    case 'midi.nrpn': {
      let nrpn: readonly unknown[] | undefined;
      if (s.param !== undefined) nrpn = paramOf(s, ctx, where).nrpn;
      else if (Array.isArray(s.nrpn)) nrpn = s.nrpn;
      if (!nrpn) throw new Error(`${where}: ${s.param ?? 'the step'} has no NRPN on this device`);
      const value14 =
        s.value14 !== undefined
          ? int(s.value14, 'value14', 0, 16383, where)
          : nrpnValue(int(s.value, 'value', 0, 127, where));
      const msb = int(nrpn[0], 'nrpn msb', 0, 127, where);
      const lsb = int(nrpn[1], 'nrpn lsb', 0, 127, where);
      return channelsOf(s, ctx, where).map((ch) => ({ op: 'midi.nrpn' as const, ch, msb, lsb, value14 }));
    }
    case 'midi.start':
      return [{ op: 'midi.start' }];
    case 'midi.stop':
      return [{ op: 'midi.stop' }];
    case 'key': {
      if (typeof s.key !== 'string' && typeof s.key !== 'number') throw new Error(`${where}: key is missing`);
      return [
        {
          op: 'key',
          key: s.key,
          ...(s.hold_ms !== undefined ? { hold_ms: int(s.hold_ms, 'hold_ms', 0, 60_000, where) } : {}),
        },
      ];
    }
    case 'turn': {
      if (typeof s.encoder !== 'string' && typeof s.encoder !== 'number')
        throw new Error(`${where}: encoder is missing`);
      return [{ op: 'turn', encoder: s.encoder, by: int(s.by ?? 1, 'by', -127, 127, where) }];
    }
    case 'setup': {
      const given = (['machine', 'filter', 'effect'] as const).filter((k) => s[k] !== undefined);
      if (given.length !== 1)
        throw new Error(`${where}: setup takes one of machine = TYPE, filter = TYPE, effect = MODULE_ID`);
      const what = given[0]!;
      const v = s[what];
      if (what === 'effect') {
        const route = s.route ?? 'default';
        if (typeof v !== 'string' || !v.trim() || /^\d+$/.test(v.trim()))
          throw new Error(`${where}: effect takes a loaded module id or module id with #row`);
        if (route !== 'default' && route !== 'master' && route !== 'send')
          throw new Error(`${where}: effect route must be default, master or send`);
        return [{ op: 'setup', what, track: 0, type: v, route }];
      }
      if (s.route !== undefined) throw new Error(`${where}: route is only valid for an effect`);
      const type = typeof v === 'string' && !/^\d+$/.test(v.trim()) ? v : int(v, what, 0, 255, where);
      if (typeof type === 'string' && what === 'filter') throw new Error(`${where}: filter takes a type number`);
      return parseTracks(s.track ?? '', where).map((track) => ({ op: 'setup' as const, what, track, type }));
    }
    case 'wait': {
      const kinds = [s.ms !== undefined, s.event !== undefined, s.drained !== undefined].filter(Boolean).length;
      if (kinds !== 1) throw new Error(`${where}: wait takes exactly one of ms, event, drained`);
      if (s.event !== undefined) {
        if (typeof s.event !== 'string') throw new Error(`${where}: event must be a log kind name`);
        const match = isTable(s.match) ? (s.match as Record<string, string | number>) : undefined;
        return [
          {
            op: 'wait',
            event: s.event,
            ...(match ? { match } : {}),
            ...(s.within_ms !== undefined ? { within_ms: int(s.within_ms, 'within_ms', 1, 3_600_000, where) } : {}),
          },
        ];
      }
      if (s.drained !== undefined) return [{ op: 'wait', drained: true }];
      return [{ op: 'wait', ms: int(s.ms, 'ms', 0, 3_600_000, where) }];
    }
    case 'shot':
      if (typeof s.name !== 'string' || !/^[A-Za-z0-9_.-]+$/.test(s.name))
        throw new Error(`${where}: shot needs a name of letters, digits, _ . -`);
      return [{ op: 'shot', name: s.name }];
    case 'capture':
      if (s.mark !== 'start' && s.mark !== 'stop') throw new Error(`${where}: capture mark is start or stop`);
      return [{ op: 'capture', mark: s.mark }];
    default:
      throw new Error(`${where}: unknown action ${s.action}`);
  }
}

/** Substitute the arguments (the file's defaults, then `args`) into `defs` and resolve every step for the device. */
export function resolveSteps(defs: readonly StepDef[], defaults: Args, args: Args, ctx: ResolveContext): Op[] {
  const all = { ...defaults, ...args };
  const out: Op[] = [];
  defs.forEach((d, i) => {
    const where = `${ctx.where} step ${i + 1} (${d.action})`;
    const s = substitute(d, all, where) as StepDef;
    out.push(...resolveOne(s, ctx, where));
  });
  return out;
}

/** A step list as a library gives it: `ref.steps` is the name of a step file. */
export type StepLibrary = Readonly<Record<string, StepFile>>;

export function resolveRef(ref: StepRef, lib: StepLibrary, ctx: ResolveContext): Op[] {
  if ('inline' in ref) return resolveSteps(parseInlineSteps(ref.inline), {}, {}, ctx);
  const f = lib[ref.steps];
  if (!f)
    throw new Error(`${ctx.where}: no step file ${JSON.stringify(ref.steps)} (known: ${Object.keys(lib).join(', ')})`);
  return resolveSteps(f.steps, f.args, ref.args ?? {}, { ...ctx, where: `${ctx.where} ${f.name}` });
}
