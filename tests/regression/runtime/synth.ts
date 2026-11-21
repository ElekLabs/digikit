import fs from 'node:fs';
import path from 'node:path';
import { COLLECT_BYTES } from '../../../packages/dk-session/src/gen_proto.ts';
import { repoRoot, type Device } from '@digi/test-support';
import { parse } from 'smol-toml';
import { deviceProfile } from './device-data.ts';
import { patchkit } from './patchkit.ts';
import { servicesSidecar } from './service-files.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

/**
 * Synthetic `.dkm` files: `patchkit synth-dkm` (packages/patchkit/src/dkmsynth.rs) builds a module of a chosen size, with a chosen number of
 * machines, parameters, pages, DSP bytes and voices from a TOML spec. This file has the typed spec, its TOML text, the packing and the geometry
 * of a device that a set of such modules is computed from. It holds no test logic.
 */

/** A voice row of the DSP section: a pinned machine type number, or a machine of the file (its hashed key). */
export interface SynthVoice {
  type?: number;
  machine?: string;
  stateBytes?: number;
  voices?: number;
}

/** The DSP section: `image` and `bss` are bytes (multiples of 4), `align` is 8, 16, 32 or 64. */
export interface SynthDsp {
  image: number;
  bss?: number;
  align?: number;
  voices?: SynthVoice[];
}

/** The fields of `dkmsynth::Spec`. */
export interface SynthSpec {
  name: string;
  /** Data bytes behind the code (a multiple of 4): makes the file larger. */
  pad?: number;
  /** The byte of `pad` and of the DSP image. */
  fill?: number;
  /** ColdFire bss bytes: not stored in the file. */
  bss?: number;
  machines?: number;
  params?: number;
  pages?: number;
  /** Allow a file over the staging buffer. */
  oversize?: boolean;
  dsp?: SynthDsp;
}

/** The TOML text of SPEC. */
export function synthToml(spec: SynthSpec): string {
  const lines = [`name = ${JSON.stringify(spec.name)}`];
  for (const key of ['pad', 'bss', 'machines', 'params', 'pages'] as const)
    if (spec[key]) lines.push(`${key} = ${spec[key]}`);
  if (spec.fill !== undefined) lines.push(`fill = ${spec.fill}`);
  if (spec.oversize) lines.push('oversize = true');
  if (spec.dsp) {
    lines.push('[dsp]', `image = ${spec.dsp.image}`);
    if (spec.dsp.bss) lines.push(`bss = ${spec.dsp.bss}`);
    if (spec.dsp.align) lines.push(`align = ${spec.dsp.align}`);
    for (const v of spec.dsp.voices ?? []) {
      lines.push('[[dsp.voice]]');
      if (v.type !== undefined) lines.push(`type = ${v.type}`);
      if (v.machine !== undefined) lines.push(`machine = ${JSON.stringify(v.machine)}`);
      if (v.stateBytes) lines.push(`state_bytes = ${v.stateBytes}`);
      if (v.voices) lines.push(`voices = ${v.voices}`);
    }
  }
  return `${lines.join('\n')}\n`;
}

/** Pack SPEC for DEVICE into DIR/FILE (default: the spec's name with `/` as `-`); returns the path. */
export async function packSynth(device: Device, spec: SynthSpec, dir: string, file?: string): Promise<string> {
  fs.mkdirSync(dir, { recursive: true });
  const out = path.join(dir, file ?? `${spec.name.replace(/\//g, '-')}.dkm`);
  const toml = `${out}.toml`;
  fs.writeFileSync(toml, synthToml(spec));
  await patchkit(['synth-dkm', toml, '--device', device, '-o', out]);
  // the answer the Nx pack target writes for a shipped module (`neededServices` reads it); this module is packed here, so the live patchkit gives it
  fs.writeFileSync(servicesSidecar(out), (await patchkit(['services', '--device', device, out])).stdout);
  return out;
}

/** The voices that name machine types by number (a DSP-only module: no machine record, no hashed key). */
export const pinned = (types: number[]): SynthVoice[] => types.map((type) => ({ type }));

/** The loader's format limits (sdk/sdk.toml `[limits.format]`, tools/device/dkproto.py). */
export const ARENA_HDR = 16; // the arena block's header
export const DSP_GRANULE = 64; // the DSP area is handed out in granules
export const DSP_SLOT_BYTES = 64; // window 0 of core/dsp-link starts with the slot block (DK_SLOT_BYTES): the area the runtime reports is that much shorter than `dsp_area_bytes`
export const DKM_HEADER = 160;
/** The header and the 8-byte header table of a module with no machine, effect or key, up to the next multiple of 16 (where the image starts). */
export const DKM_FIXTURE_HEADER = DKM_HEADER + 16;
export const MAX_VOICES = 8; // voices of one DSP section
/** The synthetic module's code: `moveq #0,d0; rts`. */
export const CODE = 4;

/** What a set of synthetic modules is computed from: the device's limits, its run-time machine type range and the arena and DSP area addresses. */
export interface Geometry {
  device: Device;
  arenaLo: number;
  arenaHi: number;
  dspArea: number;
  dspBase: number;
  stageBytes: number;
  arenaBytes: number;
  modules: number;
  machineRows: number;
  paramRows: number;
  pageRows: number;
  typeFirst: number;
  /** Offset in the DSP area of the first 8-aligned byte (the area's base is not 8-aligned on the Digitakt II). */
  dspFirst: number;
  /** Bytes of the arena's one free block at the start (header included). */
  arenaFree: number;
}

/** `dynamic_first ..= last` of the device in the machines `types.toml`: the run-time machine type numbers. */
export function dynamicRange(device: Device): [number, number] {
  const types = parse(
    fs.readFileSync(path.join(repoRoot(), 'image-modules/points/machines/types.toml'), 'utf8'),
  ) as Record<string, { dynamic_first: number; last: number }>;
  return [types[device]!.dynamic_first, types[device]!.last];
}

/** The plan must carry the symbols `DKRT_ARENA` (core/runtime) and `_dl_area` (core/dsp-link). */
export function geometryOf(device: Device, plan: RuntimePlan): Geometry {
  const profile = deviceProfile(device);
  const lim = profile.limits;
  const [first, last] = dynamicRange(device);
  if (last - first + 1 !== lim.machine_rows!)
    throw new Error(`${device}: machine_rows ${lim.machine_rows} is not the run-time type range ${first}..${last}`);
  const arenaBase = planSymbol(plan, 'DKRT_ARENA');
  const dspBase = planSymbol(plan, '_dl_area');
  const arenaLo = (arenaBase + 15) & ~15;
  const arenaHi = (arenaBase + lim.arena_bytes!) & ~15;
  return {
    device,
    arenaLo,
    arenaHi,
    dspArea: profile.digikit.dsp_area_bytes - DSP_SLOT_BYTES,
    dspBase,
    stageBytes: lim.stage_bytes!,
    arenaBytes: lim.arena_bytes!,
    modules: lim.modules!,
    machineRows: lim.machine_rows!,
    paramRows: lim.param_rows!,
    pageRows: lim.page_rows!,
    typeFirst: first,
    dspFirst: -dspBase & 7,
    arenaFree: arenaHi - arenaLo,
  };
}

/** The arena bytes a module of image + bss = N takes: its payload rounded to 16 and the header. */
export const block = (n: number): number => ((n + 15) & ~15) + ARENA_HDR;

/**
 * Every load takes a `Collect` (the builder calls of `init`) from the arena until the commit is done (runtime loader.rs `complete`): `size_of::<Collect>()`
 * of collect.rs on the ColdFire (`COLLECT_BYTES`, generated from sdk/sdk.toml and checked by the runtime at compile time). No module can take the arena to its last byte: the one that loads last leaves this block free. A module that declares machines
 * allocates its record block while the `Collect` is alive, so the `Collect`'s block is a hole before the record block when it is freed; only blocks
 * smaller than the hole fill it, and no later module can take it as part of a large one.
 */
export const COLLECT_BLOCK = block(COLLECT_BYTES);

/** `pad` of a raw module whose file is `stageBytes + extra` bytes (the file is the header, its table, the code and the pad). */
export const stageFilePad = (stageBytes: number, extra = 0): number => stageBytes + extra - DKM_FIXTURE_HEADER - CODE;

// ---- the full set: every limit used at once (capacity.device.test.ts, memaudit.device.test.ts) ---------------------------------

export const E_NOMEM = -8;
export const E_DSP_NOSPACE = -33;
export const E_DSP_ROWS = -34;
export const LOADED = 'loaded';
/** The result a module must give at boot: loaded, or the loader's error code. */
export type Want = number | typeof LOADED;

/** What a packed module takes of each resource (`patchkit needs`). */
export interface Usage {
  dspLen: number;
  dspAlign: number;
  rows: number;
  machines: number;
  params: number;
  pages: number;
}

export function usageOf(n: Record<string, number>): Usage {
  return {
    dspLen: n.DSP_AREA!,
    dspAlign: n.DSP_ALIGN!,
    rows: n.VOICE_ROWS!,
    machines: n.MACHINE_ROWS!,
    params: n.PARAM_ROWS!,
    pages: n.PAGE_ROWS!,
  };
}

/**
 * The modules that, behind the reference modules REFS (loaded first), use every limit at once, followed by three that do not fit:
 * M takes the machine types, parameter rows and page rows the references leave; V the voice rows (pinned types, behind the references' hashed ones);
 * D the DSP area, to its last whole granule; F the arena, to the last byte. F takes the largest free block `arenaLargest` (payload bytes) that a boot of the
 * references, M, V and D leaves (the providers they load, the references' and V's and D's blocks and the arena copies of their DSP images are all in it: a
 * ColdFire-only rig keeps those until a transfer that never comes), less the `Collect` block of its own load. Then X (a raw module larger than the largest free block: E_NOMEM), Y (a voice: E_DSP_ROWS) and
 * Z (a DSP module: E_DSP_NOSPACE).
 *
 * `machines: false` leaves out M, V and Y: a module that declares machines or voices needs `machines.dks`, whose DSP part needs the SHARC, so a card that stays cold
 * (no SHARC acknowledges a DSP part) fills the DSP area and the arena only. `dspHoles` are the free ranges of the DSP area that a boot of the set before D leaves, with
 * the services that boot loads (their DSP code is in the area too, so the area is no longer one range): D takes the largest to its last whole granule and Z is larger than
 * the other one (no module of one range fills two). Without `dspHoles` D takes what the area has before any service loads.
 */
export interface FullSetOptions {
  machines?: boolean;
  dspHoles?: number[];
}
export function fullSet(
  g: Geometry,
  refs: Usage[],
  arenaLargest: number,
  { machines = true, dspHoles }: FullSetOptions = {},
): { spec: SynthSpec; want: Want }[] {
  const sum = (key: keyof Usage) => refs.reduce((n, r) => n + r[key], 0);
  const typesLeft = g.machineRows - sum('machines');
  const rowsLeft = g.machineRows - sum('rows');
  const paramsLeft = g.paramRows - sum('params');
  const pagesLeft = g.pageRows - sum('pages');
  if (machines) {
    if (rowsLeft > MAX_VOICES || rowsLeft < 1 || typesLeft < 1)
      throw new Error(`the references leave ${rowsLeft} rows and ${typesLeft} types`);
    if (pagesLeft > typesLeft)
      throw new Error(`${pagesLeft} page rows left, but only ${typesLeft} machines to put them on`);
  }
  const firstPinned = g.typeFirst + sum('rows');
  // the DSP area: references, then V (one granule), then D takes every whole granule left (first fit from the area's start, each range at the
  // first address its alignment allows)
  let cursor = 0;
  for (const [length, align] of [
    ...refs.filter((r) => r.dspLen).map((r) => [r.dspLen, r.dspAlign] as const),
    ...(machines ? [[DSP_GRANULE, 8] as const] : []),
  ]) {
    const start = Math.ceil((g.dspBase + cursor) / align) * align - g.dspBase;
    cursor = start + length;
  }
  const dStart = Math.ceil((g.dspBase + cursor) / 8) * 8 - g.dspBase;
  // behind the services a boot loads, their DSP code is in the area too: the free ranges are what the runtime reports after the boot (counted from the same offset)
  const dLen =
    dspHoles === undefined
      ? Math.floor((g.dspArea - dStart) / DSP_GRANULE) * DSP_GRANULE
      : Math.floor((dspHoles[0]! - g.dspFirst) / DSP_GRANULE) * DSP_GRANULE;
  if (dLen < DSP_GRANULE) throw new Error('the references leave no DSP area for D');
  // Z needs more than the other range holds, so that it is refused for want of space and does not load into it
  const zLen = Math.max(DSP_GRANULE, Math.ceil(((dspHoles?.[1] ?? 0) + 1) / DSP_GRANULE) * DSP_GRANULE);
  const fTotal = arenaLargest - COLLECT_BLOCK; // the block of F is its payload (a multiple of 16, as the largest block's is) and the header
  if (fTotal < CODE) throw new Error('the references leave no arena for F');
  const modules = refs.length + (machines ? 7 : 4);
  if (modules > g.modules) throw new Error(`the full set needs ${modules} modules, the device has ${g.modules}`);
  const machineSet: { spec: SynthSpec; want: Want }[] = machines
    ? [
        { spec: { name: 'cap/full-m', machines: typesLeft, params: paramsLeft, pages: pagesLeft }, want: LOADED },
        {
          spec: {
            name: 'cap/full-v',
            dsp: { image: 64, voices: pinned(Array.from({ length: rowsLeft }, (_, i) => firstPinned + i)) },
          },
          want: LOADED,
        },
      ]
    : [];
  return [
    ...machineSet,
    { spec: { name: 'cap/full-d', dsp: { image: 64, bss: dLen - 64 } }, want: LOADED },
    { spec: { name: 'cap/full-f', bss: fTotal - CODE }, want: LOADED },
    // X: a raw module that needs more than the largest free block (the hole of M is smaller than the block the `Collect` takes)
    { spec: { name: 'cap/full-x', bss: 2 * COLLECT_BLOCK }, want: E_NOMEM },
    // Y is refused for want of voice rows (E_DSP_ROWS: V holds every row, and the rows are checked before the types). It pins the type just below the run-time
    // range: no reference or M or V holds it (the references take the lowest numbers of the range, V the pinned numbers above them), so no key of the set is
    // repeated and the plan (E_PLAN_KEY) lets Y through to the row check that the case names.
    ...(machines
      ? [
          {
            spec: { name: 'cap/full-y', dsp: { image: 64, voices: pinned([g.typeFirst - 1]) } },
            want: E_DSP_ROWS as Want,
          },
        ]
      : []),
    { spec: { name: 'cap/full-z', dsp: { image: 64, bss: zLen - 64 } }, want: E_DSP_NOSPACE },
  ];
}

/** The modules of `fullSet` that load before F: everything but F and the three refused ones. */
const beforeF = (set: { spec: SynthSpec; want: Want }[]) => set.filter((m) => !/^cap\/full-[fxyz]$/.test(m.spec.name));

/**
 * `fullSet` sized by a real boot: `largestAfter` boots the references and the modules of the set before F (M, V, D; the caller adds its own references)
 * and gives the `ARENA.largest` of that boot. The providers load only for the modules that need them, so only a boot tells what they hold.
 * `dspHolesAfter` (a coupled card) boots the references and the modules before D (M, V) and gives the free ranges of the DSP area of that boot (CAPACITY
 * `DSP_AREA`: `largest` is the largest range and `limit - used` the total, so the rest is what the other ranges hold): the services that boot loads keep DSP code
 * in the area, so D is sized by what is left.
 */
export async function fullSetOnBoot(
  g: Geometry,
  refs: Usage[],
  largestAfter: (before: { spec: SynthSpec; want: Want }[]) => Promise<number>,
  options: {
    machines?: boolean;
    dspHolesAfter?: (before: { spec: SynthSpec; want: Want }[]) => Promise<{ largest: number; free: number }>;
  } = {},
): Promise<{ spec: SynthSpec; want: Want }[]> {
  const machines = options.machines ?? true;
  let dspHoles: number[] | undefined;
  if (options.dspHolesAfter) {
    const first = beforeF(fullSet(g, refs, COLLECT_BLOCK + CODE, { machines })).filter(
      (m) => !m.spec.name.startsWith('cap/full-d'),
    );
    const { largest, free } = await options.dspHolesAfter(first);
    dspHoles = [largest, free - largest];
  }
  const opts = { machines, dspHoles };
  const probe = beforeF(fullSet(g, refs, COLLECT_BLOCK + CODE, opts));
  return fullSet(g, refs, await largestAfter(probe), opts);
}
