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

/** One MIDI parameter of `[midi.params]`: the 7-bit control change number and the 14-bit NRPN (`[msb, lsb]`); either may be absent. */
export interface MidiParam {
  cc?: number;
  nrpn?: readonly [number, number];
}

/** `[midi]`: the factory MIDI channel (1-16) of each track (index 0 is track 1) and the named parameters the tools set over MIDI. */
export interface MidiMap {
  trackChannels: readonly number[];
  params: Readonly<Record<string, MidiParam>>;
}

/** `[storage.slots]`: how a device with numbered sample slots takes samples (`samples = "stock-slots"`). `bytesMax`: the most audio bytes of one sample.
 * `apply`: when the device uses a change (`restart`: at its next start). */
export interface SlotStorage {
  path: string;
  count: number;
  bytesMax: number;
  rate: number;
  channels: number;
  bits: number;
  byteOrder: 'big' | 'little';
  apply: 'live' | 'restart';
}

/** `[storage]`: the backend of each kind of content. `samples`: `stock-rpc` (the device's MidiRpc file commands), `digikit` (the digikit store) or `stock-slots`
 * (the firmware's numbered slots, written through its Data API; `slots` says how). */
export interface Storage {
  samples: 'stock-rpc' | 'digikit' | 'stock-slots';
  modules: 'digikit';
  slots?: SlotStorage;
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

/**
 * The stock firmware the tools read for a device: the version of `[acceptance] firmware`, else the last `[[firmware]]` (the newest listed).
 * Its `filename` names the stock .syx.
 */
export function stockFirmware(device: Pick<DeviceRecord, 'acceptance' | 'firmware'>): Firmware | undefined {
  const version = device.acceptance?.firmware;
  return version ? device.firmware.find((f) => f.version === version) : device.firmware[device.firmware.length - 1];
}

/** `identity`: `[device]` + `[capabilities]`; `patchable`: also `[patch]`; `runtime`: also `[digikit]`. */
export type Level = 'identity' | 'patchable' | 'runtime';

export interface ParameterWatchEntry {
  scope: 'track' | 'kit';
  word: number;
}

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
  /** `[storage]`; absent when the file has none (the device has no content backend yet). */
  storage?: Storage;
  acceptance?: Acceptance;
  /** `[midi]`; absent when the file has none. */
  midi?: MidiMap;
  /** Parameter kind to raw word mapping, by verified firmware port version. */
  parameterWatches?: Readonly<Record<string, Readonly<Record<string, ParameterWatchEntry>>>>;
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

const STORAGE_WORDS = {
  samples: ['stock-rpc', 'digikit', 'stock-slots'],
  modules: ['digikit'],
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

const SLOT_KEYS = ['path', 'count', 'bytes_max', 'rate', 'channels', 'bits', 'byte_order', 'apply'] as const;

function storageSlots(file: string, t: unknown): SlotStorage {
  const s = need(file, '[storage.slots]', t, isTable);
  for (const key of Object.keys(s))
    if (!(SLOT_KEYS as readonly string[]).includes(key))
      throw new Error(`devices/${file}: [storage.slots] has an unknown key ${key}`);
  const whole = (key: string, max = 0x7fffffff): number => {
    const v = s[key];
    if (typeof v !== 'number' || !Number.isInteger(v) || v < 1 || v > max)
      throw new Error(`devices/${file}: [storage.slots] ${key} must be a whole number from 1 to ${max}`);
    return v;
  };
  const word = <T extends string>(key: string, words: readonly T[]): T => {
    const v = s[key];
    if (typeof v !== 'string' || !(words as readonly string[]).includes(v))
      throw new Error(`devices/${file}: [storage.slots] ${key} must be one of ${words.join(', ')}`);
    return v as T;
  };
  const path = s.path;
  if (typeof path !== 'string' || !/^\/[a-z][a-z0-9_-]*$/.test(path))
    throw new Error(`devices/${file}: [storage.slots] path must be a route like "/samples"`);
  const bytesMax = whole('bytes_max');
  if (bytesMax % 2 !== 0) throw new Error(`devices/${file}: [storage.slots] bytes_max must be even (16-bit samples)`);
  const channels = whole('channels');
  const bits = whole('bits');
  if (channels !== 1 || bits !== 16)
    throw new Error(`devices/${file}: [storage.slots] only mono 16-bit is supported (channels = 1, bits = 16)`);
  return {
    path,
    count: whole('count', 4096),
    bytesMax,
    rate: whole('rate'),
    channels,
    bits,
    byteOrder: word('byte_order', ['big', 'little'] as const),
    apply: word('apply', ['live', 'restart'] as const),
  };
}

function storage(file: string, t: unknown): Storage | undefined {
  if (t === undefined) return undefined;
  const s = need(file, '[storage]', t, isTable);
  const out: Table = {};
  for (const key of Object.keys(STORAGE_WORDS) as (keyof typeof STORAGE_WORDS)[]) {
    const v = s[key];
    if (typeof v !== 'string' || !(STORAGE_WORDS[key] as readonly string[]).includes(v))
      throw new Error(`devices/${file}: [storage] ${key} must be one of ${STORAGE_WORDS[key].join(', ')}`);
    out[key] = v;
  }
  for (const key of Object.keys(s))
    if (!(key in STORAGE_WORDS) && key !== 'slots')
      throw new Error(`devices/${file}: [storage] has an unknown key ${key}`);
  const slots = s.slots === undefined ? undefined : storageSlots(file, s.slots);
  if ((out.samples === 'stock-slots') !== (slots !== undefined))
    throw new Error(`devices/${file}: [storage.slots] is needed by samples = "stock-slots" and by nothing else`);
  if (slots) out.slots = slots;
  return out as unknown as Storage;
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
  // a role section may be read-only (a device with a bootstrap or peer CPU), except the MAIN OS: patchkit has to change it
  if (ro.includes(out.main)) throw new Error(`devices/${file}: [container] the main section is read-only`);
  for (const role of ['cf2', 'fpga'] as const) {
    const id = out[role];
    if (id !== undefined && !ro.includes(id))
      throw new Error(`devices/${file}: [container] ${role} section ${id} must be in readonly`);
  }
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

function midiMap(file: string, v: unknown): MidiMap | undefined {
  if (v === undefined) return undefined;
  const m = need(file, '[midi]', v, isTable);
  const inRange = (x: unknown, lo: number, hi: number): x is number => isInt(x) && x >= lo && x <= hi;
  const chans = m.track_channels;
  if (!Array.isArray(chans) || chans.length === 0 || !chans.every((c) => inRange(c, 1, 16)))
    throw new Error(`devices/${file}: [midi] track_channels must list channels 1 to 16`);
  const raw = need(file, '[midi.params]', m.params, isTable);
  const params: Record<string, MidiParam> = {};
  for (const [name, p] of Object.entries(raw)) {
    const t = need(file, `[midi.params] ${name}`, p, isTable);
    const out: MidiParam = {};
    if (t.cc !== undefined) {
      if (!inRange(t.cc, 0, 127)) throw new Error(`devices/${file}: [midi.params] ${name}: cc must be 0 to 127`);
      out.cc = t.cc;
    }
    if (t.nrpn !== undefined) {
      const n = t.nrpn;
      if (!Array.isArray(n) || n.length !== 2 || !inRange(n[0], 0, 127) || !inRange(n[1], 0, 127))
        throw new Error(`devices/${file}: [midi.params] ${name}: nrpn must be [msb, lsb] of 0 to 127`);
      out.nrpn = [n[0], n[1]];
    }
    if (out.cc === undefined && out.nrpn === undefined)
      throw new Error(`devices/${file}: [midi.params] ${name} needs cc or nrpn`);
    params[name] = out;
  }
  return { trackChannels: chans as number[], params };
}

function parameterWatches(file: string, v: unknown, firmware: readonly Firmware[]): DeviceRecord['parameterWatches'] {
  if (v === undefined) return undefined;
  const byVersion = need(file, '[parameter_watches]', v, isTable);
  return Object.fromEntries(
    Object.entries(byVersion).map(([version, watch]) => {
      const field = `[parameter_watches.${version}]`;
      if (!firmware.some((f) => f.version === version))
        throw new Error(`devices/${file}: ${field} requires a registered firmware version`);
      const mappings = Object.entries(need(file, field, watch, isTable)).map(([kind, value]) => {
        const t = need(file, `${field} ${kind}`, value, isTable);
        if (
          !/^[a-zA-Z][a-zA-Z0-9]*(\.[a-zA-Z][a-zA-Z0-9]*)*$/.test(kind) ||
          (t.scope !== 'track' && t.scope !== 'kit') ||
          !isInt(t.word) ||
          t.word < 0 ||
          t.word > 255
        )
          throw new Error(
            `devices/${file}: ${field} ${kind} needs a parameter kind, track or kit scope, and word 0 to 255`,
          );
        return [kind, { scope: t.scope, word: t.word }] as const;
      });
      return [version, Object.fromEntries(mappings)] as const;
    }),
  );
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
    const mm = midiMap(file, doc.midi);
    if (mm) rec.midi = mm;
    const watches = parameterWatches(file, doc.parameter_watches, fw);
    if (watches) rec.parameterWatches = watches;
    const st = storage(file, doc.storage);
    if (st) rec.storage = st;
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
