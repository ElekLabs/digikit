/**
 * What a module declares, for the rigs (port of the spec half of tools/patchkit/pk_conform.py): the DSP sources of a device, the entry
 * symbols, the machine type, the parameters with their knob slots and the frame layout, read from module.toml, build/gen/gallery.json
 * and devices/*.toml; and the budget of a block (`patchkit budget`, packages/dkprof).
 */
import { globSync, readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { parse } from 'smol-toml';
import { ROOT } from './rig.ts';
import { patchkit } from './toolchain.ts';
import { galleryGaps as rigGalleryGaps, hasGallery } from './gallery-gaps.mjs';
import {
  CONFORMANCE_ROOTS,
  NEGATIVE_VARIANTS,
  conformanceKind,
  dspSourcePaths,
  negativeVariantsOf,
} from './conformance-manifest.mjs';

export const DEVICES = ['dt2', 'dn2'] as const;
export type Device = (typeof DEVICES)[number];

/** Stock rows of the knob list that a DSP routine reads (a code in `knobs`, not a `[[param]]` id): name, min, max, default. 0xca is TUNE of SAMPLE. */
const STOCK_KNOBS: Record<number, [string, number, number, number]> = {
  0xca: ['tune', 0, 0x7f00, 0x4000],
};
/** Level bound of a block: full scale is +-1.0; a bound 18 dB above it flags a runaway, not a loud but valid sound. */
export const LEVEL_BOUND = 8.0;

/** Fixtures that must FAIL the suite: id -> a text of the failure. */
export const NEGATIVE: Record<string, string> = {
  'test/dyn-dsp-hang': 'did not return',
  'test/dyn-guard-nan': 'guard stepped in',
};

for (const [id, v] of Object.entries(NEGATIVE_VARIANTS)) NEGATIVE[id] = v[4];

export const FX_NEGATIVE: Record<string, string> = {
  'test/dyn-fx-nan': 'guard stepped in',
};
export class ConformError extends Error {}

export interface Param {
  id: string;
  /** position in the machine's knob list */
  knob: number;
  lo: number;
  hi: number;
  default: number;
  /** a stock knob of the base machine (the tune knob), not a `[[param]]` of the module */
  stock: boolean;
  latchedWord: number | null;
}

/** A value on the UI grid (words are UI * 256). */
export const paramMid = (p: Param): number => Math.floor((p.lo + p.hi) / 2) & ~0xff;

export interface Frame {
  base: number;
  stride: number;
  group: number;
  gap: number;
}

/** Halfword of the DSP's frame copy that holds knob j of voice v: base + stride * v + j + (j // group) * gap. */
export const frameHw = (f: Frame, v: number, j: number): number =>
  f.base + f.stride * v + j + (f.group ? Math.floor(j / f.group) * f.gap : 0);

export interface MachineSpec {
  module: string;
  device: Device;
  moddir: string;
  /** the DSP sources of the module for the device */
  sources: string[];
  entry: string;
  machineType: number;
  params: Param[];
  frame: Frame;
  /** a contract voice: the note_on routine's symbol and the `dk_machine_desc` symbol */
  noteOn: string | null;
  desc: string | null;
  /** `[conformance] fixtures` of module.toml: what the rig sets up before a case */
  fixtures: string[];
  /** `[conformance] sample_records`: the data symbol of the module's sample pool records */
  sampleRecords: string | null;
}

export const machineLabel = (s: MachineSpec): string => `${s.module}@${s.device}`;

/** What decides the behaviour of the routine: two modules built from the same sources and parameters run once. */
export const machineKey = (s: MachineSpec): string => JSON.stringify([s.device, s.sources, s.params]);

export interface FxSpec {
  module: string;
  moddir: string;
  sources: string[];
  process: string;
  desc: string;
  /** always "": the runtime reserves the state of a run-time effect (the rig does the same) */
  state: string;
  /** (min, max) of the page knob that feeds parameter word i (a run-time effect with a page) */
  knobs: [number, number][];
}

export const fxLabel = (s: FxSpec): string => `${s.module}@fx`;

type Toml = Record<string, any>; // oxlint-disable-line no-explicit-any

const tomlCache = new Map<string, Toml>();
function readToml(file: string): Toml {
  let t = tomlCache.get(file);
  if (!t) tomlCache.set(file, (t = parse(readFileSync(file, 'utf8')) as Toml));
  return t;
}
const manifest = (moddir: string): Toml => readToml(path.join(moddir, 'module.toml'));

/** Module id -> module directory: the committed roots, then PATCHKIT_MODULES (tools/patchkit/modroots.py). */
export function moduleDirs(): Map<string, string> {
  const extra = process.env.PATCHKIT_MODULES ?? '';
  const selected = process.env.DIGI_CONFORM_ROOT ?? '';
  const key = JSON.stringify([extra, selected]);
  const known = dirsCache.get(key);
  if (known) return known;
  const roots = selected
    ? [path.resolve(ROOT, selected)]
    : [...CONFORMANCE_ROOTS.map((r) => path.join(ROOT, r)), ...extra.split(path.delimiter).filter(Boolean)];
  const found = new Map<string, string>();
  for (const root of roots) {
    const files = globSync(selected ? 'module.toml' : '**/module.toml', { cwd: root }).sort((a, b) => cmpPath(a, b));
    for (const file of files) {
      const dir = path.dirname(path.join(root, file));
      const id = manifest(dir).module.id as string;
      const known = found.get(id);
      if (known) throw new ConformError(`duplicate module id ${id}: ${known} and ${dir}`);
      found.set(id, dir);
    }
  }
  dirsCache.set(key, found);
  return found;
}
const dirsCache = new Map<string, Map<string, string>>();

/** Path order of a directory walk: component by component. */
function cmpPath(a: string, b: string): number {
  const x = a.split('/');
  const y = b.split('/');
  for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i]! < y[i]! ? -1 : 1;
  return x.length - y.length;
}

/** The id of the module whose directory is `dir`. */
export function moduleIdOfDir(dir: string): string {
  const real = path.resolve(dir);
  for (const [id, d] of moduleDirs()) if (path.resolve(d) === real) return id;
  throw new ConformError(`no module in ${dir}`);
}

export function moduleDir(id: string): string {
  const dir = moduleDirs().get(id);
  if (!dir) throw new ConformError(`unknown module ${id}`);
  return dir;
}

function voices(man: Toml, n = 0): { render: string; noteOn: string; desc: string } | null {
  if (man.module?.type === 'machine' && man.machine?.length) {
    const m = man.machine[n];
    const p = m.dsp || String(m.id).replaceAll('-', '_');
    return { render: `_${p}_render`, noteOn: `_${p}_note_on`, desc: `_${p}_desc` };
  }
  return null;
}

/** (render symbol, literal type or null) of the module's voice. */
function entryRow(man: Toml, n = 0): [string, string | null] | null {
  const v = voices(man, n);
  if (v) return [v.render, null];
  for (const e of man.entry ?? [])
    if (e.registry === 'voices' && typeof e.pc === 'string') return [e.pc, e.type ?? null];
  return null;
}

function dspSources(man: Toml, moddir: string, device: string): string[] {
  return dspSourcePaths(man, device).map((source: string) => path.resolve(moddir, source));
}

function typeNumber(mid: string, man: Toml, device: string, literal: string | null, n = 0): number {
  if (literal !== null && /^[0-9][0-9a-zA-Z_]*$/.test(String(literal))) return Number(literal);
  const types = readToml(path.join(ROOT, 'image-modules/points/machines/types.toml'))[device];
  let mach: string | undefined = man.machine?.length ? man.machine[n].id : undefined;
  if (literal !== null) mach = String(literal).split('.')[1];
  const key = `${mid}:${mach}`;
  if (types.alloc && key in types.alloc) return Number(types.alloc[key]);
  if (man.module?.dynamic && 'dynamic_first' in types) return Number(types.dynamic_first);
  throw new ConformError(`${mid}: no machine type for ${key} in core/machines types.toml [${device}]`);
}

function readGallery(moddir: string, mid: string, hint: string): Toml {
  const file = path.join(moddir, 'build/gen/gallery.json');
  if (!existsSync(file)) throw new ConformError(`${mid}: no build/gen/gallery.json: run ${hint}`);
  return JSON.parse(readFileSync(file, 'utf8')) as Toml;
}

function machineParams(moddir: string, man: Toml, device: string, n = 0): Param[] {
  const mid = man.module?.id ?? path.basename(moddir);
  const gallery = readGallery(moddir, mid, '`patchkit gen`');
  const machines = gallery.machines ?? [];
  if (!machines.length) return [];
  const dev = machines[n]?.devices?.[device];
  if (!dev) return [];
  const decl = new Map<string, Toml>((gallery.params?.[device] ?? []).map((p: Toml) => [p.id, p]));
  const out: Param[] = [];
  (dev.knobs ?? []).forEach((k: unknown, j: number) => {
    if (typeof k !== 'object' || k === null) return;
    const knob = k as Toml;
    if ('param' in knob && decl.has(knob.param)) {
      const p = decl.get(knob.param)!;
      out.push({
        id: knob.param,
        knob: j,
        lo: Number(p.min),
        hi: Number(p.max),
        default: Number(p.default),
        stock: false,
        latchedWord: p.latched_word !== undefined && p.latched_word !== null ? Number(p.latched_word) : null,
      });
    } else if (knob.stock in STOCK_KNOBS) {
      const [name, lo, hi, dflt] = STOCK_KNOBS[knob.stock]!;
      out.push({ id: name, knob: j, lo, hi, default: dflt, stock: true, latchedWord: null });
    }
  });
  return out;
}

const deviceFiles = new Map<string, Toml>();
function deviceFile(short: string): Toml {
  if (deviceFiles.size === 0)
    for (const file of globSync('*.toml', { cwd: path.join(ROOT, 'devices') }).sort()) {
      const d = readToml(path.join(ROOT, 'devices', file));
      if (d.device) deviceFiles.set(d.device.short, d);
    }
  const d = deviceFiles.get(short);
  if (!d) throw new ConformError(`no device file for ${short}`);
  return d;
}

/** The spec of module MID on DEVICE, or null when the module is no machine with a DSP object for that device. */
/**
 * Problems of a machine module's `build/gen/gallery.json` for a device, as `device: text` lines. A device of `[targets]` or `ports` of
 * module.toml (or of an object's `devices`) that a machine of the gallery does not list, or lists with no parameter (unless `[uses.DEVICE] param_rows = 0` declares none), has no knob slots
 * there: the rig would run it with every parameter word at 0. `module:check` writes the gallery, so the fix is to run it again.
 */
export function galleryGaps(mid: string): string[] {
  const moddir = moduleDir(mid);
  const man = manifest(moddir);
  if (!hasGallery(man)) return [];
  return rigGalleryGaps(man, readGallery(moddir, mid, '`patchkit gen`'), 'build/gen/gallery.json');
}

export function machineSpec(mid: string, device: Device): MachineSpec | null {
  if (mid.split('/')[0] === 'core') return null; // the framework renders no machine of its own
  const variant = NEGATIVE_VARIANTS[mid];
  if (variant) {
    const base = machineSpec(mid.split('#')[0]!, device);
    if (!base) return null;
    const [file, render, noteOn, desc] = variant;
    return { ...base, module: mid, sources: [path.resolve(base.moddir, file)], entry: render, noteOn, desc };
  }
  // `module#ID` is the machine ID of a module that declares several (a calibration module: one kernel per machine); `module` is its first
  const [base, which] = mid.split('#') as [string, string | undefined];
  const moddir = moduleDir(base);
  const man = manifest(moddir);
  const n = which === undefined ? 0 : ((man.machine ?? []) as Toml[]).findIndex((m) => m.id === which);
  if (n < 0) throw new ConformError(`${mid}: the module declares no machine ${which}`);
  const ports: string[] = man.module?.ports ?? [];
  if (ports.length && !ports.some((p) => p.split('-')[0] === device)) return null;
  const row = entryRow(man, n);
  if (!row) return null;
  const sources = dspSources(man, moddir, device);
  if (!sources.length) return null;
  const f = deviceFile(device).modules.frame;
  const v = voices(man, n);
  return {
    module: mid,
    device,
    moddir,
    sources,
    entry: row[0],
    machineType: typeNumber(base, man, device, row[1], n),
    params: machineParams(moddir, man, device, n),
    frame: { base: f.base, stride: f.stride, group: f.group ?? 0, gap: f.gap ?? 0 },
    noteOn: v?.noteOn ?? null,
    desc: v?.desc ?? null,
    fixtures: [...(man.conformance?.fixtures ?? [])],
    sampleRecords: man.conformance?.sample_records ?? null,
  };
}

/** Every (machine module, device) the suite runs (NEGATIVE fixtures only with `negative`). */
export function machineSpecs(negative = false): MachineSpec[] {
  const out: MachineSpec[] = [];
  const dirs = moduleDirs();
  const add = (id: string) => {
    for (const device of DEVICES) {
      const s = machineSpec(id, device);
      if (s) out.push(s);
    }
  };
  for (const [mid, dir] of dirs)
    if (mid in NEGATIVE === negative && conformanceKind(manifest(dir)) === 'machine') add(mid);
  if (negative) for (const mid of dirs.keys()) for (const variant of negativeVariantsOf(mid)) add(variant);
  return out;
}

function fxRecords(mid: string, man: Toml): { id: string; process: string; desc: string; knobs: Toml[] }[] {
  if (man.module?.type !== 'fx' || !man.fx?.length) return [];
  const gallery = readGallery(moduleDir(mid), mid, '`module:check`');
  const effects: Toml[] = gallery.effects ?? [];
  return (man.fx as Toml[]).map((f, n) => {
    const p = f.dsp || String(f.id).replaceAll('-', '_');
    return { id: f.id, process: `_${p}_process`, desc: `_${p}_desc`, knobs: effects[n]?.knobs ?? [] };
  });
}

/** The spec of effect module MID, or null when it has no `DK_FX` record (a run-time effect) with a DSP object. */
export function fxSpec(mid: string): FxSpec | null {
  if (mid.split('/')[0] === 'core') return null;
  const moddir = moduleDir(mid);
  const man = manifest(moddir);
  const sources = () => [...new Set(DEVICES.flatMap((d) => dspSources(man, moddir, d)))];
  for (const fx of fxRecords(mid, man))
    if (typeof fx.process === 'string' && typeof fx.desc === 'string') {
      const s = sources();
      return s.length
        ? {
            module: mid,
            moddir,
            sources: s,
            process: fx.process,
            desc: fx.desc,
            state: '',
            knobs: fx.knobs.map((k) => [Number(k.min), Number(k.max)] as [number, number]),
          }
        : null;
    }
  return null;
}

/** Every effect module the FX suite runs (FX_NEGATIVE fixtures only with `negative`). */
export function fxSpecs(negative = false): FxSpec[] {
  const out: FxSpec[] = [];
  for (const [mid, dir] of moduleDirs()) {
    if (mid in FX_NEGATIVE !== negative || conformanceKind(manifest(dir)) !== 'fx') continue;
    const s = fxSpec(mid);
    if (s) out.push(s);
  }
  return out;
}

/** Effects in the chain: the most a digikit device has (devices/*.toml `[limits] fx_slots`). */
function fxMax(): number {
  deviceFile('dt2');
  const slots = [...deviceFiles.values()]
    .filter((d) => d.digikit && d.card?.store)
    .map((d) => Number(d.limits.fx_slots));
  return Math.max(...slots);
}

export interface Budget {
  voiceBlock: number;
  noteOnBlock: number;
  headroom: number;
}

let budgets:
  | { devices: Record<string, { voice_block: number; note_on_block: number; headroom: number }>; fx_budget: number }
  | undefined;
function deviceBudgets(): NonNullable<typeof budgets> {
  return (budgets ??= JSON.parse(patchkit(['budget', '--fx-max', String(fxMax())])));
}

/** The instruction budget of a voice-block (headroom / voices) and of a note-on block (the headroom) of a device. */
export function budget(device: Device): Budget {
  const d = deviceBudgets().devices[device]!;
  return { voiceBlock: d.voice_block, noteOnBlock: d.note_on_block, headroom: d.headroom };
}

/** Instructions of one process call: the headroom of the tighter device shared by the chain's effects. */
export const fxBudget = (): number => deviceBudgets().fx_budget;

/** Instructions of core/fx around one process call at most (a fade block: copy, output check, crossfade). */
export const FX_ADAPTER_MAX = 900;
/** The range of a parameter word until the manifest declares one. */
export const FX_PARAM_MAX = 0x7f00;

/** The parameter word j for the case value RAW (0 .. FX_PARAM_MAX): the raw integer, or for a knob of a page the float32 the runtime sends. */
export function fxWord(spec: FxSpec, j: number, raw: number): number {
  const knob = spec.knobs[j];
  if (!knob) return raw;
  const f = Math.fround;
  const [lo, hi] = knob;
  const v = f(f(lo) + f(f(hi) - f(lo)) * f(f(raw) / f(FX_PARAM_MAX)));
  const view = new DataView(new ArrayBuffer(4));
  view.setFloat32(0, v, true);
  return view.getUint32(0, true);
}
