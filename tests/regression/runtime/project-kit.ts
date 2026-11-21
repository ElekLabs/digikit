import fs from 'node:fs';
import zlib from 'node:zlib';
import { deviceError, LogCursor, runtimeEvent, type Clock, type DkSession, type RuntimeEvent } from '@digi/dk-session';
import { CMD, READ_MAX } from '@digi/dk-session/gen_proto';
import { button, encoder, pollUntil, type Device, type Session } from '@digi/test-support';
import { expect } from 'vitest';
import { postRaw, readSectors } from './card-kit.ts';
import { REDRAW, SETTLE, bounded, dspBytes, pickSine, word, type Rt } from './fx-live.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';
import { serviceSymbol } from './service-symbols.ts';
import { overlayRecords } from './write-safety.ts';

/**
 * Helpers of the settings, roundtrip and equiv device tests (ports of 6a5bb1ed:tools/patchkit/pk_digikit_settings.py, pk_digikit_project.py and pk_digikit_scen.py):
 * the settings record codec and mailbox, the project slot and the machine tables. The card fixture is card-rig.ts.
 * Candidates for card-kit.ts: `settingsRecord`, `settingsMailbox`.
 */

// ---- the settings record (port of tools/device/dksettings.py; vectors in modules/core/runtime/vectors/settings-vectors.json) -------------------------

const HEADER = 18;
/** One settings copy: 16 sectors (sdk/sdk.toml [settings]). */
export const SETTINGS_AREA = 16 * 512;
export const SETTINGS_KEY_MAX = 47;
export const SETTINGS_VALUE_MAX = 1024;

export type SettingsEntry = [key: string, value: Buffer];
export interface SettingsRecord {
  seq: number;
  used: number;
  entries: SettingsEntry[];
}

const crcOf = (b: Buffer, used: number) => zlib.crc32(Buffer.concat([b.subarray(0, 14), b.subarray(HEADER, used)]));

const validKey = (key: Buffer) =>
  key.length >= 1 && key.length <= SETTINGS_KEY_MAX && key.every((c) => c >= 0x21 && c <= 0x7e);

/** The entries of a record of `used` bytes, or null when one overruns it, has an illegal key, or a key is there twice. */
function entriesOf(b: Buffer, used: number): SettingsEntry[] | null {
  const out: SettingsEntry[] = [];
  let at = HEADER;
  while (at < used) {
    const keyLen = b[at]!;
    const ks = at + 1;
    if (keyLen === 0 || keyLen > SETTINGS_KEY_MAX || ks + keyLen + 2 > used) return null;
    const key = b.subarray(ks, ks + keyLen);
    const valueLen = b.readUInt16BE(ks + keyLen);
    const vs = ks + keyLen + 2;
    if (valueLen > SETTINGS_VALUE_MAX || vs + valueLen > used || !validKey(key)) return null;
    const name = key.toString('latin1');
    if (out.some(([k]) => k === name)) return null;
    out.push([name, Buffer.from(b.subarray(vs, vs + valueLen))]);
    at = vs + valueLen;
  }
  return at === used ? out : null;
}

/** `'ok'`, or the first rule the record breaks: magic, version, used, crc, entry. */
export function settingsWhy(b: Buffer): string {
  if (b.length < HEADER || b.subarray(0, 4).toString('latin1') !== 'DKST') return 'magic';
  if (b.readUInt16BE(4) !== 1) return 'version';
  const used = b.readUInt32BE(10);
  if (used < HEADER || used > SETTINGS_AREA || used > b.length) return 'used';
  if (b.readUInt32BE(14) !== crcOf(b, used)) return 'crc';
  return entriesOf(b, used) ? 'ok' : 'entry';
}

/** The record, or null when it is not a valid one. */
export function decodeSettings(b: Buffer): SettingsRecord | null {
  if (settingsWhy(b) !== 'ok') return null;
  const used = b.readUInt32BE(10);
  return { seq: b.readUInt32BE(6), used, entries: entriesOf(b, used)! };
}

/** A record of the entries in order (the runtime appends a new key at the end and keeps the order of the others). */
export function encodeSettings(seq: number, entries: SettingsEntry[]): Buffer {
  const body = Buffer.concat(
    entries.map(([k, v]) => {
      const len = Buffer.alloc(2);
      len.writeUInt16BE(v.length);
      return Buffer.concat([Buffer.from([k.length]), Buffer.from(k, 'latin1'), len, v]);
    }),
  );
  const head = Buffer.alloc(14);
  head.write('DKST', 0, 'latin1');
  head.writeUInt16BE(1, 4);
  head.writeUInt32BE(seq, 6);
  head.writeUInt32BE(HEADER + body.length, 10);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(zlib.crc32(Buffer.concat([head, body])));
  return Buffer.concat([head, crc, body]);
}

export const settingsMap = (rec: SettingsRecord | null) =>
  Object.fromEntries((rec?.entries ?? []).map(([k, v]) => [k, v.toString('hex')]));

// ---- settings through the mailbox and over USB ----------------------------------------------------------------------------------

const CMD_SETTINGS = { PUT: 9, GET: 10, REMOVE: 11 };
const MB_ARG = 12;
export const E_SET_KEY = -66;
export const E_SET_NOKEY = -68;
export const E_SET_OFF = -70;

/** The mailbox test commands CMD_SETTINGS_PUT, _GET and _REMOVE (argument: u8 key_len, key, u8 value_len, value). */
export function settingsMailbox(session: Session, plan: RuntimePlan) {
  const arg = (key: string, value: Buffer = Buffer.alloc(0)) =>
    Buffer.concat([Buffer.from([key.length]), Buffer.from(key), Buffer.from([value.length]), value]);
  return {
    put: (key: string, value: Buffer) => postRaw(session, plan, CMD_SETTINGS.PUT, arg(key, value)),
    remove: (key: string) => postRaw(session, plan, CMD_SETTINGS.REMOVE, arg(key)),
    /** (the result, the value): the runtime answers the value's length and leaves the bytes at the start of the mailbox argument. */
    async get(key: string): Promise<[number, Buffer]> {
      const n = await postRaw(session, plan, CMD_SETTINGS.GET, arg(key));
      if (n <= 0) return [n, Buffer.alloc(0)];
      const mb = planSymbol(plan, 'DKRT_MAILBOX');
      const bytes = Buffer.alloc((n + 3) & ~3);
      for (let i = 0; i < bytes.length; i += 4) bytes.writeUInt32BE(await word(session, mb + MB_ARG + i), i);
      return [n, bytes.subarray(0, n)];
    },
  };
}

/** SETTINGS (read only) in pieces of 768 bytes: the flags (bit 0: changes the card does not hold yet), the card's current copy (0 A, 1 B, 2 none), its seq and the record bytes. Throws a DkError with the device status. */
export async function settingsRecord(dk: Pick<DkSession, 'request'>) {
  let raw = Buffer.alloc(0);
  let total: number | null = null;
  let head = { flags: 0, copy: 2, seq: 0 };
  while (total === null || raw.length < total) {
    const body = Buffer.alloc(6);
    body.writeUInt32BE(raw.length, 0);
    body.writeUInt16BE(READ_MAX, 4);
    const reply = await dk.request(CMD.SETTINGS, body);
    if (reply.status !== 0) throw deviceError('SETTINGS', reply.status, reply.body);
    const b = Buffer.from(reply.body);
    if (b.length <= 10) throw new Error('SETTINGS: malformed or empty reply');
    const piece = { flags: b[0]!, copy: b[1]!, seq: b.readUInt32BE(2) };
    total ??= b.readUInt32BE(6);
    if (raw.length === 0) head = piece;
    else expect(piece, 'the record did not change while it was read').toEqual(head);
    raw = Buffer.concat([raw, b.subarray(10)]);
  }
  return { ...head, raw };
}

/** One sector of a card file, zero padded past its end. */
export const sectorOf = (card: string, sector: number) => readSectors(card, sector, 1);

// ---- the panel ---------------------------------------------------------------------------------------------------------------------

/** Turn an encoder (Python `do_turn` + `run_ms`). */
export async function panelTurn(rt: Rt, enc: string, by: number, ms: number) {
  await bounded(() => rt.session.client.inputTurn(encoder(enc), by));
  await rt.ui.wait({ ms: 10 + ms, why: SETTLE });
}

/** Hold or release a key (Python `emu.button`). */
export const panelKey = (session: Session, key: string, down: boolean) =>
  bounded(() => session.client.inputButton(button(key), down));

// ---- the machine-type assignment ------------------------------------------------------------------------------------------------------

/** The 24-bit hash of a run-time machine's global id `<module id>:<machine id>`: the low 24 bits of FNV-1a 32. */
export function machineHash(module: string, machine: string): number {
  let h = 0x811c9dc5;
  for (const b of Buffer.from(`${module}:${machine}`)) h = Math.imul(h ^ b, 0x01000193) >>> 0;
  return h & 0xffffff;
}

/** The type number the machines service assigned to `<module>:<machine>` in this boot (`DKRT_ASSIGN` of `machines.dks`: 3 header words, 8 map words, then 64 module indices, 64 keys, 64 types). */
export async function dynType(session: Session, plan: RuntimePlan, module: string, machine: string): Promise<number> {
  const base = await serviceSymbol({ session }, plan, 'machines', 'DKRT_ASSIGN');
  const count = await word(session, base + 4);
  const want = machineHash(module, machine);
  for (let i = 0; i < count; i++) {
    const key = await word(session, base + 4 * (75 + i));
    const type = await word(session, base + 4 * (139 + i));
    if (key >>> 8 === want && type) return type;
  }
  throw new Error(`${module}:${machine} has no assigned type`);
}

// ---- the panel drive of a project (port of pk_digikit_project.py: select_dt2, build_project, save_project, load_project) ----------------

/** A run-time machine to put on a track: its assigned type (`dynType`) and its position `hint` in the machine list. */
export interface PanelMachine {
  track: number;
  type: number;
  hint: number;
}

/** Choose the machine of list position `hint` on the selected track until the track holds `type` (the DT2 list position shifts by one with the stock machines). */
export async function selectDt2(rt: Rt, track: number, type: number, hint: number): Promise<void> {
  const { session } = rt;
  const seen: [number, number][] = [];
  // The first SRC list after a project save may not take (the keys go to the save's leftovers), so the hint is tried up to four times;
  // every try starts from the top of the list (12 UP presses), so it does not depend on the machine the track holds.
  for (const downs of [hint, hint, hint, hint, hint + 1, hint - 1]) {
    await pickSine(rt, downs, 12);
    seen.push([downs, await trackType(session, track)]);
    if (seen.at(-1)![1] === type) return;
  }
  throw new Error(`no list position gives type ${type} on track ${track}: (downs, type) ${JSON.stringify(seen)}`);
}

/** Put the machine of type `type` on `track` from the list cursor where it stands (no UP presses, so the setup stays short): the position `hint` and its neighbours are tried until the track holds `type` (Python `select_type`). */
export async function selectType(rt: Rt, track: number, type: number, hint: number): Promise<number> {
  const { session } = rt;
  const seen: [number, number][] = [];
  for (const downs of [hint, hint + 1, hint - 1, hint + 2, hint + 3]) {
    await pickSine(rt, downs);
    seen.push([downs, await trackType(session, track)]);
    if (seen.at(-1)![1] === type) return downs;
  }
  throw new Error(`no list position gives type ${type} on track ${track}: (downs, type) ${JSON.stringify(seen)}`);
}

/** The panel script that makes a project: the machines on their tracks with two parameter values, then on track 1 a note trig with a p-lock and a sound lock. */
export async function buildPanelProject(
  rt: Rt,
  machines: PanelMachine[],
  { lockTurn = 11, soundLockTurn = 2 } = {},
): Promise<void> {
  const { session } = rt;
  const dt2 = session.options.device === 'dt2';
  for (const m of machines) {
    if (m.track !== 1) await rt.ui.tap(`TRIG ${m.track}`, { until: { ms: 300, why: REDRAW }, held: 'TRK' });
    if (dt2) await selectDt2(rt, m.track, m.type, m.hint);
    else await pickSine(rt, m.hint);
    await rt.ui.tap(dt2 ? 'SRC' : 'SYN', { until: { ms: 300, why: REDRAW } });
    for (const [enc, by] of [
      ['ENC A', 9],
      ['ENC B', -7],
    ] as const)
      await panelTurn(rt, enc, by, 100);
  }
  if (machines.some((m) => m.track !== 1)) await rt.ui.tap('TRIG 1', { until: { ms: 300, why: REDRAW }, held: 'TRK' });
  // a trig, a p-lock and a sound lock on it: GRID RECORDING, tap the trig, hold it and turn knobs
  await rt.ui.tap('RECORD', { until: { ms: 200, why: REDRAW } });
  await rt.ui.tap('TRIG 1', { until: { ms: 200, why: REDRAW } });
  await panelKey(session, 'TRIG 1', true);
  await rt.ui.wait({ ms: 100, why: SETTLE });
  await panelTurn(rt, 'ENC C', lockTurn, 100);
  await panelTurn(rt, 'LEVEL', soundLockTurn, 300);
  await panelKey(session, 'TRIG 1', false);
  await rt.ui.wait({ ms: 300, why: SETTLE });
  await rt.ui.tap('RECORD', { until: { ms: 200, why: REDRAW } });
}

// ---- the runtime log, for waits on the project service ---------------------------------------------------------------------------------

/** Protocol calls on the paused emulator (`CardRig.usb` of a runtime). */
export type ProtocolRun = <T>(run: (dk: DkSession) => Promise<T>) => Promise<T>;

/**
 * The events of the runtime log kept in a list, so a test can look at an event after a wait for another one (`RuntimeEvents.until` drops what it passes). `since` of every call is a
 * `mark()` taken before the action. Module messages (`dk_log` of a module, MOD_LOG) arrive only for a module whose log level is raised: `watch` names those modules (by id) and
 * `raise()` sets them again after a load, which gives the module a new record.
 */
export class ProjectLog {
  private readonly cursor: LogCursor;
  private readonly list: RuntimeEvent[] = [];
  private readonly usb: ProtocolRun;
  private readonly clock: Clock;
  private readonly watch: string[];

  private constructor(usb: ProtocolRun, clock: Clock, watch: string[]) {
    this.usb = usb;
    this.clock = clock;
    this.watch = watch;
    this.cursor = new LogCursor({ logPage: (from, quick) => usb((dk) => dk.logPage(from, quick)) });
  }

  /** The log from the newest event on. */
  static async open(usb: ProtocolRun, clock: Clock, watch: string[] = []): Promise<ProjectLog> {
    const log = new ProjectLog(usb, clock, watch);
    await log.cursor.fromNewest();
    await log.raise();
    return log;
  }

  /** The log of a boot that just started: from its first event. */
  static async openBoot(usb: ProtocolRun, clock: Clock, watch: string[] = []): Promise<ProjectLog> {
    const log = new ProjectLog(usb, clock, watch);
    log.cursor.fromBoot();
    await log.raise();
    return log;
  }

  /** The log level of the watched modules that are loaded goes to DEBUG (3); the other levels stay. */
  async raise(): Promise<void> {
    if (!this.watch.length) return;
    await this.usb(async (dk) => {
      const config = await dk.logConfig();
      const levels = [...config.modules];
      for (const m of await dk.modules())
        if (this.watch.includes(m.name) && m.index < levels.length) levels[m.index] = 3;
      await dk.setLogConfig(config.level, levels);
    });
  }

  /** Read what is new; true when the cursor saw a new boot (the list then holds only the new boot's events). */
  async pump(): Promise<boolean> {
    const r = await this.cursor.read();
    if (r.lost > 0) throw new Error(`the runtime log lost ${r.lost} events before they were read`);
    if (r.newBoot) this.list.length = 0;
    this.list.push(...r.events.map(runtimeEvent));
    return r.newBoot;
  }

  /** The place in the list to pass as `since`: after everything read so far. */
  async mark(): Promise<number> {
    await this.pump();
    return this.list.length;
  }

  /** The events of `kind` (all that match) from `since` on, as read so far (call `pump` or a wait first). */
  all(kind: string, match: (e: RuntimeEvent) => boolean = () => true, since = 0): RuntimeEvent[] {
    return this.list.slice(since).filter((e) => e.kind === kind && match(e));
  }

  /** `MOD_LOG` lines of a module message (`dk_log` text) from `since` on: the `value` of each. */
  messages(msg: string, since = 0): number[] {
    return this.all('MOD_LOG', (e) => e.fields.msg === msg, since).map((e) => Number(e.fields.value) >>> 0);
  }

  /** The first event of `kind` that matches, from `since` on; waits in emulated time. */
  async until(
    kind: string,
    match: (e: RuntimeEvent) => boolean,
    { since = 0, withinMs = 30_000, what = kind }: { since?: number; withinMs?: number; what?: string } = {},
  ): Promise<RuntimeEvent> {
    const found = await pollUntil(
      this.clock,
      async () => {
        await this.pump();
        return this.all(kind, match, since)[0];
      },
      (e) => e !== undefined,
      { withinMs, message: () => `the event ${what} (seen from ${since}: ${this.summary(since)})`, intervalMs: 50 },
    );
    return found!;
  }

  /** A short account of the events read from `since` on: the count of each kind, then the last few events. */
  summary(since = 0): string {
    const tail = this.list.slice(since);
    const counts = new Map<string, number>();
    for (const e of tail) counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1);
    const last = tail
      .filter((e) => e.kind.startsWith('PROJECT_'))
      .concat(tail.slice(-3))
      .map((e) => `${e.kind} ${JSON.stringify(e.fields)}`);
    return `${tail.length} events [${[...counts].map(([k, n]) => `${k} x${n}`).join(', ')}]; last: ${last.join(' | ') || 'none'}`;
  }

  /** The events seen so far from `since` on, one per line (for a failure message). */
  seen(since = 0): string {
    return this.list
      .slice(since)
      .map((e) => `#${e.seq} ${e.kind} ${JSON.stringify(e.fields)}`)
      .join('\n');
  }
}

/** `PROJECT_SAVED`, `PROJECT_LOADED` of `slot`. */
export const forSlot = (slot: number) => (e: RuntimeEvent) => Number(e.args[0]) === slot;

/** The FNV-1a (32 bits) of a module id, the `id_hash` of PROJECT_MODULE (`name_hash` of the SDK). */
export function idHash(id: string): number {
  let h = 0x811c9dc5;
  for (const b of Buffer.from(id)) h = Math.imul(h ^ b, 0x01000193) >>> 0;
  return h;
}

/** The paths of the store below `projects/` (the files of the slots and `projects/archive/`). */
export const projectPaths = async (usb: ProtocolRun): Promise<string[]> =>
  (await usb((dk) => dk.list('projects/'))).map((e) => e.path);

/** The store path of the file of `slot` (`projects/<slot as 3 digits>-<name>.dkp`) among `paths`, or undefined. */
export const slotFile = (paths: string[], slot: number): string | undefined =>
  paths.find((p) => p.startsWith(`projects/${String(slot).padStart(3, '0')}-`) && p.endsWith('.dkp'));

/** A whole store file, read in pieces of `READ_MAX`. */
export async function readStoreFile(dk: Pick<DkSession, 'read'>, path: string): Promise<Buffer> {
  let out = Buffer.alloc(0);
  for (;;) {
    const r = await dk.read(path, out.length, READ_MAX);
    out = Buffer.concat([out, r.bytes]);
    if (out.length >= r.size || r.bytes.length === 0) return out.subarray(0, r.size);
  }
}

/** The fields of the header of a `.dkp` (docs/design/digikit-project.md section 4) and its sections (offset 42 to `used`). */
export function dkpOf(file: Buffer) {
  const used = file.readUInt32BE(34);
  const nameLen = file[9]!;
  return {
    slot: file[8]!,
    name: file.subarray(10, 10 + nameLen).toString('latin1'),
    stamp: file.readUInt32BE(26),
    counter: file.readUInt32BE(30),
    used,
    sections: file.subarray(42, used),
  };
}

/** A module's live job that ends: UNLOAD of `modulePath`, then wait (emulated time) until the job counter `DKRT_BOOT + 252` moved and the job is done. The job needs the DSP, and the paused protocol's short steps do not reach its end (as in the reload round-trip test). */
export async function unloadLive(rt: Rt, usb: ProtocolRun, plan: RuntimePlan, modulePath: string): Promise<void> {
  const reloads = planSymbol(plan, 'DKRT_BOOT') + 252;
  const before = await word(rt.session, reloads);
  await usb((dk) => dk.unload(modulePath));
  const end = await pollUntil(
    rt.clock,
    async () => ({ st: await usb((dk) => dk.reloadState()), n: await word(rt.session, reloads) }),
    (v) => v.st.phase === 2 && v.n > before,
    { withinMs: 20_000, message: `the unload of ${modulePath} finished` },
  );
  expect({ result: end.st.result, unloaded: end.st.unloaded }, `the unload of ${modulePath}`).toEqual({
    result: 0,
    unloaded: true,
  });
}

// ---- the project menus of the panel ------------------------------------------------------------------------------------------------------

/**
 * Options of the project flows. `events`: with the project service loaded, the flow waits on its event (PROJECT_SAVED, PROJECT_LOADED) instead of a fixed time. `from`: the slot of the
 * current project (default 0): SAVE PROJECT AS and LOAD PROJECT open with the cursor on it (on the first slot when there is none); MANAGE PROJECTS always opens on the first slot. Slots are the
 * firmware's numbers (0 to 127, the events' slot): the list shows slot 0 as 001.
 */
export interface ProjectFlow {
  events?: ProjectLog;
  from?: number;
}

/** The stock save and load dialogs stay up a moment after the service's event (it is logged at the end of the firmware's work, before the dialog closes): emulated ms to let them close before the next key. */
const DIALOG_MS = 2000;

/** Move the list cursor from slot `from` to slot `to` with UP and DOWN (the lists do not wrap). */
async function slotCursor(rt: Rt, from: number, to: number): Promise<void> {
  for (let i = 0; i < Math.abs(to - from); i++) {
    await rt.ui.tap(to > from ? 'DOWN' : 'UP', { until: { drained: true } });
    await rt.ui.wait({ ms: 80, why: REDRAW });
  }
}

/**
 * SETTINGS, PROJECT, then item `item` of it (0 LOAD PROJECT, 1 SAVE PROJECT AS, 2 MANAGE PROJECTS) opened. The PROJECT menu keeps its cursor between visits and does not wrap, so
 * two UP presses first put it on LOAD PROJECT.
 */
async function openProjectItem(rt: Rt, item: number): Promise<void> {
  await rt.ui.tap('SETTINGS', { until: { ms: 300, why: REDRAW } });
  await rt.ui.tap('YES', { until: { ms: 500, why: REDRAW } }); // PROJECT
  for (let i = 0; i < 2; i++) await rt.ui.tap('UP', { until: { ms: 200, why: REDRAW } });
  for (let i = 0; i < item; i++) await rt.ui.tap('DOWN', { until: { ms: 200, why: REDRAW } });
  await rt.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
}

/**
 * Leave a project list for the main screen: NO five times. Three reach the main screen from a list (list, PROJECT, SETTINGS); a message still shown after a paste takes one more;
 * a NO on the main screen does nothing, so the extra presses are safe.
 */
async function leaveMenus(rt: Rt): Promise<void> {
  for (let i = 0; i < 5; i++) await rt.ui.tap('NO', { until: { ms: 300, why: REDRAW } });
}

/**
 * In MANAGE PROJECTS (which always opens on the first slot, whatever the current project): open the command panel of `slot` (RIGHT) and take command `index` (0 CLEAR, 1 DELETE,
 * 2 RENAME, 3 LOAD FROM, 4 SAVE TO, 5 TOGGLE lock, 6 PURGE ALL). The panel stays open after a command; `closeManage` closes it (LEFT) and leaves the menus.
 */
async function manage(rt: Rt, slot: number, index: number): Promise<void> {
  await openProjectItem(rt, 2);
  await slotCursor(rt, 0, slot);
  await rt.ui.tap('RIGHT', { until: { ms: 500, why: REDRAW } });
  for (let i = 0; i < index; i++) await rt.ui.tap('DOWN', { until: { ms: 200, why: REDRAW } });
  await rt.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
}

async function closeManage(rt: Rt): Promise<void> {
  await rt.ui.tap('LEFT', { until: { ms: 300, why: REDRAW } });
  await leaveMenus(rt);
}

/**
 * Save the current project through FUNC+SETTINGS. A project that has no slot yet gets the slot list (cursor on 001), YES opens the name editor and YES saves; a project with a slot
 * gets "SAVE ALL PATTERNS AND PROJ SETTINGS? Y/N", YES saves and the second YES lands on the main screen, where it does nothing. `from` is the slot the save goes to (default 0).
 * With `events` the end is PROJECT_SAVED of that slot (the event is returned); without, a fixed wait.
 */
export async function savePanelProject(
  rt: Rt,
  { events, from = 0 }: ProjectFlow = {},
): Promise<RuntimeEvent | undefined> {
  const since = await events?.mark();
  await rt.ui.tap('SETTINGS', { until: { ms: 300, why: REDRAW }, held: 'FUNC' });
  await rt.ui.tap('YES', { until: { ms: 500, why: REDRAW } }); // the slot (opens the name editor), or the confirmation (saves)
  await rt.ui.tap('YES', { until: { drained: true } }); // the name editor: save; or nothing on the main screen
  if (events) {
    const saved = await events.until('PROJECT_SAVED', forSlot(from), { since, what: `PROJECT_SAVED slot ${from}` });
    await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE });
    return saved;
  }
  // No state shows the end of the write: the card overlay of a repeated save may be byte-equal, and the firmware has no save-done word. Measure before shortening.
  await rt.ui.wait({ ms: 3500, why: SETTLE });
  return undefined;
}

/** Load slot 001 through SETTINGS; with `health` (default) check the session has no fault afterwards. With `events` the end is PROJECT_LOADED of slot 1. */
export async function loadPanelProject(
  rt: Rt,
  { health = true, events }: { health?: boolean } & ProjectFlow = {},
): Promise<RuntimeEvent | undefined> {
  const { session } = rt;
  const since = await events?.mark();
  await rt.ui.tap('SETTINGS', { until: { ms: 300, why: REDRAW } });
  for (let i = 0; i < 3; i++) await rt.ui.tap('YES', { until: { ms: 500, why: REDRAW } });
  await rt.ui.tap('YES', { until: { drained: true } });
  // No firmware word or runtime event marks the end of `loadProjectFromMmc` (only the popup text); callers read the loaded state afterwards. Measure before shortening.
  let loaded: RuntimeEvent | undefined;
  if (events) {
    loaded = await events.until('PROJECT_LOADED', forSlot(1), { since, what: 'PROJECT_LOADED slot 1' });
    await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE });
  } else await rt.ui.wait({ ms: 6000, why: SETTLE });
  if (health) await bounded(() => session.expectNoFault('project loaded'));
  return loaded;
}

// The flows below follow the stock menus as seen on screen (the same on both devices): SETTINGS > PROJECT holds LOAD PROJECT, SAVE PROJECT AS, MANAGE
// PROJECTS. SAVE PROJECT AS and MANAGE PROJECTS list all 128 slots; LOAD PROJECT lists the saved projects only, then CREATE NEW. A slot number is the firmware's (0 to 127),
// as in the log events; the list shows slot 0 as 001.

/**
 * SAVE PROJECT AS slot `slot`: the slot, YES (the name editor with the project's name), YES. With `events` the end is PROJECT_SAVED of the slot. The flow ends on the main screen
 * after the save returns to it.
 */
export async function saveProjectAs(
  rt: Rt,
  slot: number,
  { events, from = 0 }: ProjectFlow = {},
): Promise<RuntimeEvent | undefined> {
  const since = await events?.mark();
  await openProjectItem(rt, 1);
  await slotCursor(rt, from, slot);
  await rt.ui.tap('YES', { until: { ms: 500, why: REDRAW } }); // the slot: opens the name editor
  await rt.ui.tap('YES', { until: { drained: true } }); // the name editor: save (the save returns to the PROJECT menu)
  let saved: RuntimeEvent | undefined;
  if (events) {
    saved = await events.until('PROJECT_SAVED', forSlot(slot), { since, what: `PROJECT_SAVED slot ${slot}` });
    await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE });
  } else await rt.ui.wait({ ms: 3500, why: SETTLE });
  await leaveMenus(rt);
  return saved;
}

/**
 * LOAD PROJECT slot `slot`. The list holds the saved projects in slot order (`saved`), the cursor starts on the current one (`from`), and YES loads at once (no confirmation when
 * nothing is unsaved). With `events` the end is PROJECT_LOADED of the slot.
 */
export async function loadProject(
  rt: Rt,
  slot: number,
  { events, from = 0, saved }: ProjectFlow & { saved: readonly number[] },
): Promise<RuntimeEvent | undefined> {
  const rows = [...saved].sort((a, b) => a - b);
  const at = rows.indexOf(from);
  const to = rows.indexOf(slot);
  if (at < 0 || to < 0)
    throw new Error(`loadProject: slots ${from} and ${slot} must be in the saved list ${rows.join(',')}`);
  const since = await events?.mark();
  await openProjectItem(rt, 0);
  await slotCursor(rt, at, to);
  await rt.ui.tap('YES', { until: { drained: true } }); // the project: loads
  let loaded: RuntimeEvent | undefined;
  if (events) {
    loaded = await events.until('PROJECT_LOADED', forSlot(slot), { since, what: `PROJECT_LOADED slot ${slot}` });
    await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE });
  } else await rt.ui.wait({ ms: 6000, why: SETTLE });
  await leaveMenus(rt);
  return loaded;
}

/**
 * RENAME of the project in slot `slot`: MANAGE PROJECTS, the slot, RIGHT, RENAME, the PROJECT NAME editor, SETTINGS for a random name, YES. The test reads the new name from the
 * firmware or the store after the next save (the store follows the save, not the rename).
 */
export async function renameProject(rt: Rt, slot: number): Promise<void> {
  await manage(rt, slot, 2);
  await rt.ui.tap('SETTINGS', { until: { ms: 300, why: REDRAW } }); // a random name
  await rt.ui.tap('YES', { until: { drained: true } });
  await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE }); // the rename writes the slot header
  await closeManage(rt);
}

/**
 * COPY of slot `source` and PASTE to slot `to` in MANAGE PROJECTS: FUNC+RECORD on the source ("COPY PROJECT"), FUNC+STOP on the destination, YES to "Replace ... CONFIRM Y/N".
 */
export async function copyPasteProject(rt: Rt, source: number, to: number): Promise<void> {
  await openProjectItem(rt, 2);
  await slotCursor(rt, 0, source);
  await rt.ui.tap('RECORD', { until: { ms: 500, why: REDRAW }, held: 'FUNC' });
  await slotCursor(rt, source, to);
  await rt.ui.tap('STOP', { until: { ms: 500, why: REDRAW }, held: 'FUNC' });
  await rt.ui.tap('YES', { until: { drained: true } }); // Replace ... CONFIRM Y/N
  await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE }); // the paste runs with a progress dialog that takes the keys
  await leaveMenus(rt);
}

/** CLEAR of slot `slot`: MANAGE PROJECTS, the slot, RIGHT, CLEAR, YES to "CLEAR '<name>'? Y/N". */
export async function clearProject(rt: Rt, slot: number): Promise<void> {
  await manage(rt, slot, 0);
  await rt.ui.tap('YES', { until: { drained: true } }); // the confirmation
  await rt.ui.wait({ ms: DIALOG_MS, why: SETTLE }); // the clear writes the card with a progress dialog
  await closeManage(rt);
}

/** Choose each of the first `positions` entries of the machine list on the selected track and turn the encoders of its page. */
export async function sweepMachineList(rt: Rt, positions: number): Promise<void> {
  const { session } = rt;
  const key = session.options.device === 'dt2' ? 'SRC' : 'SYN';
  for (let k = 0; k < positions; k++) {
    await pickSine(rt, k, positions + 2);
    await rt.ui.tap(key, { until: { ms: 200, why: REDRAW } });
    for (const [enc, by] of [
      ['ENC A', 5],
      ['ENC B', -3],
      ['ENC C', 4],
    ] as const)
      await panelTurn(rt, enc, by, 80);
  }
}

// ---- the project slot (roundtrip) ----------------------------------------------------------------------------------------------------

export const SLOT0 = 0x58000;
const SLOT_SECTORS = 0x8000;
const SECTOR = 512;
/** soundStorage_v3: magic, version 3. */
const SOUND_HEAD = Buffer.from('beefbace00000003', 'hex');
/** The one word of the slot that the first save of a project sets differently from every later save (finding 39): offset 0x18, 4 bytes. */
export const STAMP: [number, number][] = [[0x18, 4]];
/** The machine type byte of a sound record (DT2 0xd2, DN2 0xf4). */
export const typeAt = (device: Device) => (device === 'dt2' ? 0xd2 : 0xf4);
/** Word of DT2 track 1 whose third byte is the machine type; the tracks lie 0x450 apart (finding 02). */
export const TRACK_OBJ_WORD = 0x4265338c;

/** The project-slot sectors of an overlay file, with the bytes the guest wrote laid over zeros. */
export function slotSectors(overlayFile: string): Map<number, Buffer> {
  const out = new Map<number, Buffer>();
  for (const { sector, mask, data } of overlayRecords(fs.readFileSync(overlayFile))) {
    if (sector < SLOT0 || sector >= SLOT0 + 128 * SLOT_SECTORS) continue;
    const buf = Buffer.alloc(SECTOR);
    for (let i = 0; i < SECTOR; i++) if (mask[i >> 3]! & (1 << (i & 7))) buf[i] = data[i]!;
    out.set(sector, buf);
  }
  return out;
}

export const slotImage = (card: string, touched: Iterable<number>) =>
  new Map([...touched].sort((a, b) => a - b).map((s) => [s, readSectors(card, s, 1)] as const));

/** (byte offset of the record, machine type) for each sound record in the written sectors (runs of adjacent sectors are joined). */
export function soundRecords(blob: Map<number, Buffer>, at: number): [number, number][] {
  const out: [number, number][] = [];
  let run: number[] = [];
  const flush = () => {
    const data = Buffer.concat(run.map((k) => blob.get(k)!));
    for (let i = data.indexOf(SOUND_HEAD); i >= 0; i = data.indexOf(SOUND_HEAD, i + 1))
      if (i + at < data.length) out.push([run[0]! * SECTOR + i, data[i + at]!]);
  };
  for (const s of [...blob.keys()].sort((a, b) => a - b)) {
    if (run.length && s !== run.at(-1)! + 1) {
      flush();
      run = [];
    }
    run.push(s);
  }
  if (run.length) flush();
  return out;
}

/** The part of the project slot a byte offset (from the slot start) falls in (finding 02). */
function regionOf(offset: number, device: Device): string {
  const kit = 0x200 + 128 * 0x15c00;
  const pool = device === 'dt2' ? 0xda5a04 : 0xc4b004 - 130 * 0x167;
  return offset < 0x200 ? 'header' : offset < kit ? 'pattern' : offset < pool ? 'kit' : 'pool';
}

/** Compare two slot images sector by sector: the sectors only one has and the bytes that differ, by region. `ignore`: byte ranges of the first slot that are left out. */
export function diffSlot(
  a: Map<number, Buffer>,
  b: Map<number, Buffer>,
  device: Device,
  ignore: [number, number][] = [],
) {
  const onlyFirst = [...a.keys()].filter((s) => !b.has(s));
  const onlySecond = [...b.keys()].filter((s) => !a.has(s));
  const skip = new Set(ignore.flatMap(([off, n]) => Array.from({ length: n }, (_, i) => SLOT0 * SECTOR + off + i)));
  const regions: Record<string, number> = {};
  for (const s of [...a.keys()].filter((k) => b.has(k)).sort((x, y) => x - y)) {
    const x = a.get(s)!;
    const y = b.get(s)!;
    let n = 0;
    for (let i = 0; i < SECTOR; i++) if (x[i] !== y[i] && !skip.has(s * SECTOR + i)) n++;
    if (n) {
      const region = regionOf(((s - SLOT0) % SLOT_SECTORS) * SECTOR, device);
      regions[region] = (regions[region] ?? 0) + n;
    }
  }
  return {
    onlyFirst: onlyFirst.length,
    onlySecond: onlySecond.length,
    differBytes: regions,
    equal: !onlyFirst.length && !onlySecond.length && !Object.keys(regions).length,
  };
}

/** (offset in pattern 0, byte of A, byte of B) for the bytes that differ in the first pattern record of slot 0 (16 tracks of trig, p-lock and sound-lock data). */
export function pattern0Changes(
  a: Map<number, Buffer>,
  b: Map<number, Buffer>,
  n = 0x15c00,
): [number, number, number][] {
  const out: [number, number, number][] = [];
  for (let k = 0; k < Math.floor(n / SECTOR) + 1; k++) {
    const sec = SLOT0 + Math.floor((0x200 + k * SECTOR) / SECTOR);
    const x = a.get(sec) ?? Buffer.alloc(SECTOR);
    const y = b.get(sec) ?? Buffer.alloc(SECTOR);
    for (let i = 0; i < SECTOR; i++)
      if (x[i] !== y[i]) out.push([sec * SECTOR - SLOT0 * SECTOR + i - 0x200, x[i]!, y[i]!]);
  }
  return out.filter((o) => o[0] < n);
}

/** The machine type byte of a DT2 track's sound. */
export async function trackType(session: Session, track: number): Promise<number> {
  return ((await word(session, TRACK_OBJ_WORD + 0x450 * (track - 1))) >>> 8) & 0xff;
}

// ---- tables and PCM (equiv) ------------------------------------------------------------------------------------------------------------

/** The NUL-terminated string at `addr`, read by words. */
async function cstr(session: Session, addr: number, limit = 40): Promise<string> {
  if (!addr) return '';
  const out: number[] = [];
  for (let k = 0; k < limit; k += 4) {
    const w = await word(session, addr + k);
    out.push(w >>> 24, (w >>> 16) & 255, (w >>> 8) & 255, w & 255);
    if (out.includes(0)) break;
  }
  const end = out.indexOf(0);
  return Buffer.from(end < 0 ? out : out.slice(0, end)).toString('latin1');
}

export interface Tables {
  machines: Record<string, unknown>;
  params: Record<string, unknown>;
  pages: { name: string; title: string; machine: string; knobs: (string | number)[]; level: number; slots: number[] }[];
}

/** The machine, parameter and page rows with a name, decoded without pointers or type numbers (parameters are named, knobs name their parameter, pages name their machine). */
export async function decodeTables(session: Session, plan: RuntimePlan): Promise<Tables> {
  const w = (a: number) => word(session, a);
  const sym = (name: string) => planSymbol(plan, name);
  // `*.count`, `*.first` and `machines.row_size` are constant symbols: their value is the number.
  const rows = sym('machines.rows');
  const count = sym('machines.count');
  const first = sym('machines.first');
  const size = sym('machines.row_size');
  const pf = sym('pf_table');
  const pfirst = sym('params.first');
  const pcount = sym('params.count');
  const pnames = new Map<number, string>();
  const params: Record<string, unknown> = {};
  for (let k = 0; k < pcount; k++) {
    const row = pf + 60 * (pfirst + k);
    const name = await cstr(session, await w(row + 40));
    if (!name) continue;
    pnames.set(pfirst + k, name);
    const words = await Promise.all(Array.from({ length: 15 }, (_, i) => w(row + 4 * i)));
    params[name] = {
      short: await cstr(session, words[12]!),
      mirror: words[1],
      min: words[2],
      max: words[3],
      default: words[4],
      template_words: [0, 5, 6, 7, 8, 9, 11, 13, 14].map((i) => words[i]),
    };
  }
  const knob = (c: number) => pnames.get(c) ?? c;
  const mnames = new Map<number, string>();
  const machines: Record<string, unknown> = {};
  for (let k = 0; k < count; k++) {
    const row = rows + size * k;
    const name = await cstr(session, await w(row + 56));
    if (!name) continue;
    mnames.set(first + k, name);
    const f = await Promise.all(Array.from({ length: 9 }, (_, i) => w(row + 8 + 4 * i)));
    machines[name] = {
      short: await cstr(session, await w(row + 60)),
      clone_of: await w(row + 44),
      own_dsp: await w(row + 52),
      knobs: f.slice(0, 8).map(knob),
      level: f[8],
    };
  }
  const pages: Tables['pages'] = [];
  let pageRows = 0;
  let pageCount = 0;
  try {
    pageRows = sym('pages.rows');
    pageCount = sym('pages.count');
  } catch {
    // the Digitakt II has no page rows
  }
  for (let k = 0; k < pageCount; k++) {
    const row = pageRows + 84 * k;
    const machine = await w(row + 44);
    const name = await cstr(session, await w(row));
    if (!name || !mnames.has(machine)) continue;
    pages.push({
      name,
      title: await cstr(session, await w(row + 4)),
      machine: mnames.get(machine)!,
      knobs: await Promise.all(Array.from({ length: 8 }, (_, i) => w(row + 8 + 4 * i).then(knob))),
      level: await w(row + 8 + 32),
      slots: await Promise.all(Array.from({ length: 9 }, (_, i) => w(row + 48 + 4 * i))),
    });
  }
  return { machines, params, pages };
}

/** Differences between two decoded table sets (empty: equal). */
export function diffTables(a: Tables, b: Tables): string[] {
  const out: string[] = [];
  for (const kind of ['machines', 'params'] as const)
    for (const name of [...new Set([...Object.keys(a[kind]), ...Object.keys(b[kind])])].sort())
      if (JSON.stringify(a[kind][name]) !== JSON.stringify(b[kind][name]))
        out.push(`${kind.slice(0, -1)} ${name}: ${JSON.stringify(a[kind][name])} != ${JSON.stringify(b[kind][name])}`);
  const key = (p: Tables['pages'][number]) => `${p.machine}\0${p.name}`;
  const sorted = (t: Tables) => [...t.pages].sort((x, y) => (key(x) < key(y) ? -1 : key(x) > key(y) ? 1 : 0));
  if (JSON.stringify(sorted(a)) !== JSON.stringify(sorted(b)))
    out.push(`pages: ${JSON.stringify(sorted(a))} != ${JSON.stringify(sorted(b))}`);
  return out;
}

/** The type word of a free voice row of the DSP registry. */
export const FREE_TYPE = 0xffff_ffff;

/** The (type, pc) words of the eight voice rows of the DSP registry; a row is 24 words, `first` comes from the dyndsp info (word 6). */
export async function voiceRows(session: Session, plan: RuntimePlan): Promise<[number, number][]> {
  const rows = planSymbol(plan, 'voices.rows');
  const first = await word(session, planSymbol(plan, 'dyndsp_info') + 4 * 6);
  const out: [number, number][] = [];
  for (let k = 0; k < 8; k++) {
    const raw = await dspBytes(session, rows + 96 * (first + k), 8);
    out.push([raw.readUInt32LE(0), raw.readUInt32LE(4)]);
  }
  return out;
}
