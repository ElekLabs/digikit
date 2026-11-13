// The device registry for TS and JS consumers: the facts of `devices/*.toml` (docs/design/device-data.md) that code outside
// patchkit needs. Pure: `parse` takes the file texts, the loaders (`node.ts`, `vite.ts`) bring them. patchkit's `devdata.rs` is the
// validator of the tables themselves; this file checks the words and the links between the tables it reads.
import { parse as parseToml } from 'smol-toml';

/** A device id: the `[device] short` of a device file (`dt2`). `Registry.device` validates it. */
export type Device = string;

export interface Capabilities {
  dsp: 'sharc' | 'none';
  seal: 'hmac' | 'none';
  sample_pool: 'stock' | 'ddr' | 'none';
  card: 'path-fs' | 'slots' | 'none' | 'unknown';
}

/** `[container]`: section ids by role; roles a device lacks are absent. */
export interface Container {
  main: number;
  dsp?: number;
  cf2?: number;
  fpga?: number;
  readonly: readonly number[];
}

/** `[acceptance]`: the device's test cell. */
export interface Acceptance {
  firmware: string;
  readyDir: string;
  sampleCard: boolean;
}

export interface Firmware {
  version: string;
  sha256: string;
  filename?: string;
  port: boolean;
  /** The MAIN OS section hash and the contracts the boot oracle runs under; all three or none. */
  boot?: { mainSha256: string; contract: string; symbolProfile: string };
}

/** `identity`: `[device]` + `[capabilities]`; `patchable`: also `[patch]`; `runtime`: also `[digikit]`. */
export type Level = 'identity' | 'patchable' | 'runtime';

export interface DeviceRecord {
  id: Device;
  /** The file name stem of the device file (for example `some-device`): it pairs the device with `<stem>-leds.toml` and with a faceplate file. */
  stem: string;
  name: string;
  level: Level;
  /** `[device] syx_id`, the transport id of the firmware framing. */
  syxId?: number;
  capabilities: Capabilities;
  container: Container;
  acceptance?: Acceptance;
  firmware: readonly Firmware[];
  /** `[patch_sets]`: the module ids of each named set (`device`, `emulator`); empty when the file has none. */
  patchSets: Readonly<Record<string, readonly string[]>>;
  /** `[digikit] default_modules`: the module ids the apps install after a flash; empty when the file has none. */
  defaultModules: readonly string[];
}

export interface Registry {
  /** In file-name order. */
  readonly devices: readonly DeviceRecord[];
  readonly ids: readonly Device[];
  /** The ids of the devices of level `runtime` (they run the digikit runtime: module lists, patch sets). */
  readonly runtimeIds: readonly Device[];
  /** `id` when it is a known device, else an error that lists the known ids. */
  device(id: string): Device;
  get(id: string): DeviceRecord;
}

const WORDS = {
  dsp: ['sharc', 'none'],
  seal: ['hmac', 'none'],
  sample_pool: ['stock', 'ddr', 'none'],
  card: ['path-fs', 'slots', 'none', 'unknown'],
} as const;

type Table = Record<string, unknown>;

const isTable = (v: unknown): v is Table => typeof v === 'object' && v !== null && !Array.isArray(v);

function need<T>(file: string, what: string, v: unknown, ok: (v: unknown) => v is T): T {
  if (!ok(v)) throw new Error(`devices/${file}: ${what} is missing or has the wrong type`);
  return v;
}
const isString = (v: unknown): v is string => typeof v === 'string' && v !== '';
const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v);

function capabilities(file: string, t: unknown): Capabilities {
  const c = need(file, '[capabilities]', t, isTable);
  const out: Table = {};
  for (const key of Object.keys(WORDS) as (keyof typeof WORDS)[]) {
    const v = c[key];
    if (typeof v !== 'string' || !(WORDS[key] as readonly string[]).includes(v))
      throw new Error(`devices/${file}: [capabilities] ${key} must be one of ${WORDS[key].join(', ')}`);
    out[key] = v;
  }
  for (const key of Object.keys(c))
    if (!(key in WORDS)) throw new Error(`devices/${file}: [capabilities] has an unknown key ${key}`);
  return out as unknown as Capabilities;
}

function container(file: string, t: unknown, caps: Capabilities): Container {
  const c = need(file, '[container]', t, isTable);
  const main = need(file, '[container] main', c.main, isInt);
  const out: Container = { main, readonly: [] };
  for (const role of ['dsp', 'cf2', 'fpga'] as const)
    if (c[role] !== undefined) out[role] = need(file, `[container] ${role}`, c[role], isInt);
  const ro = c.readonly ?? [];
  if (!Array.isArray(ro) || !ro.every(isInt))
    throw new Error(`devices/${file}: [container] readonly must list integers`);
  out.readonly = ro;
  const roles = [out.main, out.dsp, out.cf2, out.fpga].filter((r): r is number => r !== undefined);
  if (new Set(roles).size !== roles.length) throw new Error(`devices/${file}: [container] two roles share a section`);
  // a role section may be read-only (Syntakt), except the MAIN OS: patchkit has to change it
  if (ro.includes(out.main)) throw new Error(`devices/${file}: [container] the main section is read-only`);
  if ((caps.dsp === 'sharc') !== (out.dsp !== undefined))
    throw new Error(`devices/${file}: [container] dsp is present exactly when [capabilities] dsp = "sharc"`);
  return out;
}

function firmware(file: string, t: unknown): Firmware[] {
  if (t === undefined) return [];
  if (!Array.isArray(t)) throw new Error(`devices/${file}: [[firmware]] must be an array of tables`);
  return t.map((f) => {
    const w = need(file, '[[firmware]]', f, isTable);
    const out: Firmware = {
      version: need(file, '[[firmware]] version', w.version, isString),
      sha256: need(file, '[[firmware]] sha256', w.sha256, isString).toLowerCase(),
      port: w.port === true,
    };
    if (typeof w.filename === 'string') out.filename = w.filename;
    const bootKeys = [w.main_sha256, w.boot_contract, w.symbol_profile];
    if (bootKeys.some((v) => v !== undefined)) {
      if (!bootKeys.every(isString))
        throw new Error(`devices/${file}: [[firmware]] main_sha256, boot_contract and symbol_profile go together`);
      out.boot = {
        mainSha256: (w.main_sha256 as string).toLowerCase(),
        contract: w.boot_contract as string,
        symbolProfile: w.symbol_profile as string,
      };
    }
    return out;
  });
}

function patchSets(file: string, t: unknown): Record<string, string[]> {
  if (t === undefined) return {};
  const sets = need(file, '[patch_sets]', t, isTable);
  const out: Record<string, string[]> = {};
  for (const [name, ids] of Object.entries(sets)) {
    if (!Array.isArray(ids) || !ids.every(isString))
      throw new Error(`devices/${file}: [patch_sets] ${name} must list module ids`);
    out[name] = ids;
  }
  return out;
}

function defaultModules(file: string, t: unknown): string[] {
  const ids = isTable(t) ? t.default_modules : undefined;
  if (ids === undefined) return [];
  if (!Array.isArray(ids) || !ids.every(isString))
    throw new Error(`devices/${file}: [digikit] default_modules is not a list of module ids`);
  return ids;
}

function acceptance(file: string, t: unknown, fw: readonly Firmware[]): Acceptance | undefined {
  if (t === undefined) return undefined;
  const a = need(file, '[acceptance]', t, isTable);
  const out: Acceptance = {
    firmware: need(file, '[acceptance] firmware', a.firmware, isString),
    readyDir: need(file, '[acceptance] ready_dir', a.ready_dir, isString),
    sampleCard: need(file, '[acceptance] sample_card', a.sample_card, (v): v is boolean => typeof v === 'boolean'),
  };
  if (fw.length > 0 && !fw.some((f) => f.version === out.firmware))
    throw new Error(`devices/${file}: [acceptance] firmware ${out.firmware} is not a [[firmware]] version`);
  return out;
}

/** The registry of the device files `files` (name to text). A file without `[device]` (the LED maps) is skipped. */
export function parse(files: Readonly<Record<string, string>>): Registry {
  const devices: DeviceRecord[] = [];
  for (const file of Object.keys(files).sort()) {
    let doc: Table;
    try {
      doc = parseToml(files[file] as string) as Table;
    } catch (e) {
      throw new Error(`devices/${file}: ${e instanceof Error ? e.message : String(e)}`);
    }
    if (doc.device === undefined) continue;
    const d = need(file, '[device]', doc.device, isTable);
    const id = need(file, '[device] short', d.short, isString);
    const caps = capabilities(file, doc.capabilities);
    const fw = firmware(file, doc.firmware);
    const rec: DeviceRecord = {
      id,
      stem: file.replace(/\.toml$/, ''),
      name: need(file, '[device] name', d.name, isString),
      level: doc.digikit !== undefined ? 'runtime' : doc.patch !== undefined ? 'patchable' : 'identity',
      capabilities: caps,
      container: container(file, doc.container, caps),
      firmware: fw,
      patchSets: patchSets(file, doc.patch_sets),
      defaultModules: defaultModules(file, doc.digikit),
    };
    if (d.syx_id !== undefined) rec.syxId = need(file, '[device] syx_id', d.syx_id, isInt);
    const acc = acceptance(file, doc.acceptance, fw);
    if (acc) rec.acceptance = acc;
    if (doc.digikit !== undefined && doc.patch === undefined)
      throw new Error(`devices/${file}: [digikit] needs [patch]: a runtime target must be patchable`);
    if (doc.patch !== undefined && rec.syxId === undefined)
      throw new Error(`devices/${file}: [patch] needs [device] syx_id`);
    if (devices.some((o) => o.id === id)) throw new Error(`devices/${file}: device ${id} is described twice`);
    devices.push(rec);
  }
  const ids = devices.map((x) => x.id);
  const get = (id: string): DeviceRecord => {
    const r = devices.find((x) => x.id === id);
    if (!r) throw new Error(`unknown device ${JSON.stringify(id)} (known: ${ids.join(', ')})`);
    return r;
  };
  const runtimeIds = devices.filter((x) => x.level === 'runtime').map((x) => x.id);
  return { devices, ids, runtimeIds, device: (id) => get(id).id, get };
}
