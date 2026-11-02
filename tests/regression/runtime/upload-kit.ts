import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import {
  bytesOfHex,
  concat,
  be32,
  DkError,
  DkSession,
  EmuUsbTransport,
  frame,
  hexOf,
  parseReply,
  type DkEntry,
} from '@digi/dk-session';
import { button, repoRoot, resolveInputs, Session, type Device } from '@digi/test-support';
import { parse } from 'smol-toml';
import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import { expectNoGuardHits } from '../support.ts';
import { CMD as GCMD, LOAD_STATUS, STATUS } from '../../../packages/dk-session/src/gen_proto.ts';
import { buildCard, plusdriveBinary, pokeKnob, storeList, type CardOptions } from './card-kit.ts';
import { cardStore, deviceProfile } from './device-data.ts';
import { advance, bounded, frames, options, rpc } from './fx-live.ts';
import { DEVICES, expectRuntimeUi, FIRMWARE, FULL_TIER, record } from './live.ts';
import { planSymbol, prepareRuntimePlanOffline, type RuntimePlan } from './runtime-plan.ts';
import { applyOverlay, overlayRecords, runtimeSafety, runtimeWatchEnv } from './write-safety.ts';

/**
 * Helpers of the module-upload device tests (tools/patchkit/pk_digikit_upload.py): one reusable runtime daemon per test, a cold boot per
 * card (`boot`), the card as the plusdrive reader sees it, and the upload rules of the write safety (`common_checks_upload`).
 */

const exec = promisify(execFile);
/** The ColdFire-only daemon of the Python `cf_only` rigs. Set to 'sharc' if the reusable runtime does not boot without a DSP. */
const CF_ONLY_DSP: 'zero' | 'sharc' = 'zero';
/** The test knob `DKRT_STORE_CAP` keeps every sector from here to header B out of the allocations (a DN2 store of 20 GiB then fits INFO's 32 bits). */
export const CAP_SECTORS = 0x40000;
export { GCMD };

export const sha = (b: Uint8Array): string => createHash('sha256').update(b).digest('hex');
export const shaBytes = (b: Uint8Array): Uint8Array => createHash('sha256').update(b).digest();
const text = new TextEncoder();
/** A NUL-terminated path. */
export const cstr = (p: string): Uint8Array => concat(text.encode(p), Uint8Array.of(0));
export const beginBody = (p: string, size: number, hash: Uint8Array = new Uint8Array(32)): Uint8Array =>
  concat(cstr(p), be32(size), hash);
export const dataBody = (offset: number, part: Uint8Array): Uint8Array => concat(be32(offset), part);

/** Deterministic bytes with every value (the 8-in-7 packing needs the high bits): mulberry32 of `seed`. */
export function blob(n: number, seed: number): Buffer {
  const out = Buffer.alloc(n);
  let a = seed >>> 0;
  for (let i = 0; i < n; i++) {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), a | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    out[i] = (t ^ (t >>> 14)) & 0xff;
  }
  return out;
}

/** The runtime version of sdk/sdk.toml, packed major << 16 | minor << 8 | patch (INFO's `runtime`). */
export function runtimeVersion(): number {
  const toml = parse(fs.readFileSync(path.join(repoRoot(), 'sdk/sdk.toml'), 'utf8')) as { runtime: { version: string } };
  const [major, minor, patch] = toml.runtime.version.split('.').map(Number);
  return (major! << 16) | (minor! << 8) | patch!;
}

/** The module store in absolute card sectors, with the sectors a store may use (`limit`) and the first sector of the firmware's own project area. */
export function uploadStore(device: Device) {
  const profile = deviceProfile(device);
  const project = (profile.card as unknown as { regions?: { role: string; sector: number }[] }).regions?.find(
    (r) => r.role === 'working-project',
  );
  if (!project) throw new Error(`${device}: no working-project card region`);
  const store = cardStore(device);
  return { ...store, limit: profile.card.store.header_b_sector - store.first, firmwareFirst: project.sector };
}

// ---- files, the device's LIST and the card ------------------------------------------------------------------------------------------

export type Files = Record<string, Buffer>;
type Listing = Record<string, [number, string]>;

/** {path: [size, sha256]} of the files sent. */
export const listing = (files: Files): Listing =>
  Object.fromEntries(Object.entries(files).map(([p, b]) => [p, [b.length, sha(b)]]));
export const deviceListing = async (dk: DkSession, prefix?: string): Promise<Listing> =>
  Object.fromEntries((await dk.list(prefix)).map((e: DkEntry) => [e.path, [e.size, e.sha256]]));
export const clearFiles = (files: Files): void => {
  for (const p of Object.keys(files)) delete files[p];
};

/** A device status as a number (0 for success); other errors are rethrown. The generalized `putStatus` of card-kit. */
export async function statusOf(op: () => Promise<unknown>): Promise<number> {
  try {
    await op();
    return 0;
  } catch (e) {
    const row =
      e instanceof DkError && e.kind === 'device'
        ? [...STATUS, ...LOAD_STATUS].find(([, name]) => name === e.status)
        : undefined;
    if (!row) throw e;
    return row[0];
  }
}

/** BEGIN, DATA and END with the given hash in BEGIN; the first status that is not 0. */
export async function putWithHash(rig: UploadRig, storePath: string, data: Buffer, hash: Uint8Array): Promise<number> {
  let r = await rig.request(GCMD.BEGIN, beginBody(storePath, data.length, hash));
  for (let off = 0; r.status === 0 && off < data.length; off += 32768)
    r = await rig.request(GCMD.DATA, dataBody(off, data.subarray(off, off + 32768)));
  return r.status === 0 ? (await rig.request(GCMD.END)).status : r.status;
}

async function run(file: string, args: string[]) {
  try {
    const r = await exec(file, args, { cwd: repoRoot(), maxBuffer: 64 << 20 });
    return { code: 0, stdout: r.stdout, stderr: r.stderr };
  } catch (e) {
    const x = e as { code?: number; stdout?: string; stderr?: string };
    return { code: typeof x.code === 'number' ? x.code : 1, stdout: x.stdout ?? '', stderr: x.stderr ?? String(e) };
  }
}

/** The files of a card's store as `plusdrive store ls --cat` reads them (their sha256 is checked there), and the reader's header line. A card the reader cannot read gives no files. */
export async function cardView(cardFile: string, device: Device): Promise<{ files: Files; head: string }> {
  let store: Awaited<ReturnType<typeof storeList>>;
  try {
    store = await storeList(cardFile, device);
  } catch (e) {
    return { files: {}, head: `error: ${String(e).slice(0, 200)}` };
  }
  const files: Files = {};
  for (const entry of store.entries) {
    if (entry.dir) continue;
    try {
      const cat = await exec(plusdriveBinary(), ['store', 'ls', cardFile, '--device', device, '--cat', entry.path], {
        cwd: repoRoot(),
        encoding: 'buffer',
        maxBuffer: 1 << 30,
      });
      files[entry.path] = cat.stdout;
    } catch {
      files[entry.path] = Buffer.from('<unreadable>');
    }
  }
  return { files, head: store.head };
}

const walk = (dir: string, base = dir): Files =>
  Object.assign(
    {},
    ...fs
      .readdirSync(dir, { withFileTypes: true })
      .map((e) =>
        e.isDirectory()
          ? walk(path.join(dir, e.name), base)
          : { [path.relative(base, path.join(dir, e.name))]: fs.readFileSync(path.join(dir, e.name)) },
      ),
  );

// ---- the PCM of a note --------------------------------------------------------------------------------------------------------------

/** Interleaved samples of an IEEE-float WAV (format 3). */
export function readWav(file: string): Float32Array {
  const d = fs.readFileSync(file);
  let pos = 12;
  let format = 0;
  while (pos + 8 <= d.length) {
    const tag = d.toString('latin1', pos, pos + 4);
    const size = d.readUInt32LE(pos + 4);
    if (tag === 'fmt ') format = d.readUInt16LE(pos + 8);
    else if (tag === 'data') {
      if (format !== 3) throw new Error(`${file}: expected an IEEE float WAV, format ${format}`);
      const out = new Float32Array(Math.floor(Math.min(size, d.length - pos - 8) / 4));
      for (let i = 0; i < out.length; i++) out[i] = d.readFloatLE(pos + 8 + 4 * i);
      return out;
    }
    pos += 8 + size + (size & 1);
  }
  throw new Error(`${file}: no data chunk`);
}

/** (onset of `run` minus onset of `ref`, number of samples that differ from the onsets on over the common length); the onset is the first sample above 1e-6. */
export function alignedCompare(ref: Float32Array, run: Float32Array) {
  const onset = (x: Float32Array) => x.findIndex((v) => Math.abs(v) > 1e-6);
  const a = onset(ref);
  const b = onset(run);
  if (a < 0 || b < 0) return { shift: null, differ: -1 };
  const n = Math.min(ref.length - a, run.length - b);
  let differ = 0;
  for (let i = 0; i < n; i++) if (ref[a + i] !== run[b + i]) differ++;
  return { shift: b - a, differ };
}

/** Python `M.pcm_check`: equal, or equal sample for sample from the onsets on with an even onset shift of at most `maxShift` samples. */
export function pcmMatches(ref: Float32Array, run: Float32Array, maxShift: number) {
  const exact = ref.length === run.length && ref.every((v, i) => v === run[i]);
  const { shift, differ } = alignedCompare(ref, run);
  const ok = exact || (shift !== null && shift % 2 === 0 && Math.abs(shift) <= maxShift && differ === 0);
  return { ok, detail: `${run.length} samples, exact ${exact}, onset shift ${shift}, ${differ} differ after alignment` };
}

export const peak = (x: Float32Array): number => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0);

/**
 * Play TRIG 1 from DSP frame `trigger` (the same instruction after the block boundary in every run) and capture 1500 frames to `file`. `during`
 * runs once after the key is down, inside the capture. The key stays down for the whole capture. Python `Rig.note`.
 */
export async function playNote(session: Session, file: string, trigger: number, during?: () => Promise<void>) {
  const current = await frames(session);
  if (current >= trigger - 20) throw new Error(`note setup took until frame ${current}; raise the trigger frame ${trigger}`);
  await rpc(session, 'pcm.tap', { action: 'start' });
  try {
    await bounded(() => session.client.runUntil({ frames: trigger, timeout_wall_s: 30 }));
    await bounded(() => session.client.step(44_000, { timeout_wall_s: 30 }));
    await rpc(session, 'pcm.tap', { action: 'capture', path: file });
    await bounded(() => session.client.inputButton(button('TRIG 1'), true));
    await during?.();
    await bounded(() => session.client.runUntil({ frames: trigger + 1500, timeout_wall_s: 30 }));
  } finally {
    try {
      await rpc(session, 'pcm.tap', { action: 'capture_stop' });
    } finally {
      await bounded(() => session.client.inputButton(button('TRIG 1'), false));
    }
  }
  await advance(session, 50);
  await bounded(() => session.expectNoFault('note'));
}

// ---- raw USB MIDI: the messages that are not ours ------------------------------------------------------------------------------------

/** Hand one message to the emulated USB input (high-speed delivery) and run 16 M ColdFire instructions (Python: 40 steps of 400,000). */
export async function usbSend(session: Session, msg: Uint8Array): Promise<void> {
  await rpc(session, 'midi.usb.in', { bytes: hexOf(msg), chunk: 384, period: 16_500 });
  await bounded(() => session.client.step(16_000_000, { timeout_wall_s: 120 }));
}
/** Everything the firmware sent since the last take. */
export async function usbTake(session: Session): Promise<Uint8Array> {
  const { hex } = await rpc<{ hex?: string }>(session, 'midi.usb.out');
  return bytesOfHex(hex ?? '');
}

// ---- the daemon and its cards -------------------------------------------------------------------------------------------------------

/**
 * One reusable runtime daemon for a test. `boot(card)` cold-boots the patched runtime on a clone of the card, pauses it and returns a protocol
 * session that steps the emulator while it waits for a reply. Every overlay the test exports is checked by `finish` (write-watch, flush check,
 * and the card sectors an upload may write).
 */
export class UploadRig {
  readonly device: Device;
  readonly session: Session;
  readonly plan: RuntimePlan;
  /** Artifacts of the test: the plan, overlays, card images. */
  readonly dir: string;
  readonly store: ReturnType<typeof uploadStore>;
  readonly overlays: string[] = [];
  readonly epochs = new Set<number>();
  /** The clone of the card the current boot runs on (the guest's writes are in the daemon's overlay, not in this file). */
  image = '';
  dk!: DkSession;
  transport!: EmuUsbTransport;
  private watchOutput: string;
  private cap: number | undefined;
  private boots = 0;
  private open = false;

  private constructor(device: Device, session: Session, plan: RuntimePlan, dir: string, watchOutput: string) {
    this.device = device;
    this.session = session;
    this.plan = plan;
    this.dir = dir;
    this.watchOutput = watchOutput;
    this.store = uploadStore(device);
  }

  static async start(device: Device, name: string, symbols: string[], dsp: 'zero' | 'sharc'): Promise<UploadRig> {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), `digi-${name}-${device}-`));
    console.info('upload artifacts:', dir);
    const prepared = await prepareRuntimePlanOffline({ device, firmware: FIRMWARE[device] }, dir, dir, [
      'DKRT_BOOT',
      ...symbols,
    ]);
    const watchOutput = path.join(dir, 'writewatch.jsonl');
    const env = runtimeWatchEnv(prepared.planFile, watchOutput);
    record(dir, 'watch-environment', env);
    const session = await Session.start({ ...options(device), dsp, runtime: { reuse: true }, env });
    const rig = new UploadRig(device, session, prepared.plan, dir, watchOutput);
    rig.epochs.add((await bounded(() => session.status())).epoch as number);
    return rig;
  }

  /** A card with the store of `o.modules`, seeded with the stock firmware's project as the Python `Env.card`. */
  card(name: string, o: Partial<CardOptions> = {}): Promise<string> {
    return buildCard(this.device, {
      dir: this.dir,
      name,
      syx: resolveInputs(this.device, FIRMWARE[this.device]).syx,
      ...o,
    });
  }

  /** Cold boot on a clone of `card`, wait for the main UI, pause and settle 300 ms. */
  async boot(card: string): Promise<DkSession> {
    await this.leave();
    const previous = this.image && path.dirname(this.image);
    const image = path.join(this.session.runDir, `boot${++this.boots}`, 'card.img');
    fs.mkdirSync(path.dirname(image), { recursive: true });
    fs.copyFileSync(card, image, fs.constants.COPYFILE_FICLONE);
    const boot = planSymbol(this.plan, 'DKRT_BOOT');
    const epoch = await this.session.reloadRuntimeBaseline({
      cardImage: image,
      guard: { protect: [{ lo: boot, hi: boot + 8, label: 'runtime boot identity' }], allow_pc: [] },
    });
    this.epochs.add(epoch);
    await expectRuntimeUi(this.session, this.plan, this.dir, { afterEpoch: epoch - 1 });
    await expect
      .poll(
        async () => {
          const status = await bounded(() => this.session.status());
          return { pending: status.cf.input_pending, scheduled: status.scheduled_inputs };
        },
        { timeout: 30_000, interval: 100, message: 'boot input has finished before pausing' },
      )
      .toEqual({ pending: 0, scheduled: 0 });
    await rpc(this.session, 'run.mode', { mode: 'paused' });
    await advance(this.session, 300);
    if (previous) fs.rmSync(previous, { recursive: true, force: true });
    this.image = image;
    this.cap = undefined;
    this.open = true;
    this.transport = new EmuUsbTransport((method, params) => this.session.client.call(method, params), {
      stepInstructions: 400_000,
    });
    this.dk = new DkSession(this.transport, { timeoutMs: 60_000 });
    return this.dk;
  }

  /** The end of a boot: the card writes (for the safety check), the guard and no fault. */
  private async leave(): Promise<void> {
    if (!this.open) return;
    this.open = false;
    this.dk.cancelPending();
    await this.overlay(`boot${this.boots}-end`);
    await expectNoGuardHits(this.session);
    await this.session.expectNoFault(`upload boot ${this.boots}`);
  }

  /** The guest's card writes of this boot, exported (and kept for `finish`). */
  async overlay(tag: string): Promise<string> {
    const file = path.join(this.dir, `${tag}.overlay.bin`);
    await rpc(this.session, 'card.overlay', { path: file });
    this.overlays.push(file);
    return file;
  }
  /** The sectors the guest wrote in this boot. */
  async writes(tag: string): Promise<Set<number>> {
    return new Set(Array.from(overlayRecords(fs.readFileSync(await this.overlay(tag))), (r) => r.sector));
  }
  /** The card of this boot with the guest's writes laid over it: the state a power cut now leaves. */
  async cardAfter(tag: string): Promise<string> {
    const out = path.join(this.dir, `${tag}.img`);
    applyOverlay(this.image, await this.overlay(tag), out);
    return out;
  }
  /** The files of the store as the plusdrive reader reads them from the card after the guest's writes. */
  async view(tag: string) {
    const after = await this.cardAfter(tag);
    try {
      return await cardView(after, this.device);
    } finally {
      fs.rmSync(after, { force: true });
    }
  }
  /** `plusdrive store ls` of the card after the guest's writes. */
  async ls(tag: string) {
    const after = await this.cardAfter(tag);
    try {
      return await storeList(after, this.device);
    } finally {
      fs.rmSync(after, { force: true });
    }
  }

  /** Take a store of more than 4 GiB down to `CAP_SECTORS` on this boot (DKRT_STORE_CAP). */
  async applyCap(): Promise<void> {
    if (this.store.limit * 512 > 0xffffffff) {
      await pokeKnob(this.session, this.plan, 'DKRT_STORE_CAP', CAP_SECTORS);
      this.cap = CAP_SECTORS;
    }
  }
  /** INFO's free bytes of a card with no store: used 1, room for a one-entry table (cut by the cap). */
  freeFresh(): number {
    return Math.min(((this.cap ?? this.store.limit) - 2) * 512, 0xffffffff);
  }

  /** One request and its reply, whatever the status. `version` sends the request raw with that protocol version byte. */
  async request(cmd: number, body: Uint8Array = new Uint8Array(), version?: number): Promise<{ status: number; body: Uint8Array }> {
    if (version === undefined) return this.dk.request(cmd, body);
    const seq = 0x55;
    await this.transport.send(frame(cmd, seq, body, version));
    for (;;) {
      const raw = await this.transport.recv(30_000);
      if (!raw) throw new Error('no reply to the raw request');
      const reply = parseReply(raw);
      if (reply && reply.seq === seq && reply.cmd === cmd) return reply;
    }
  }
  /** The status of one request. */
  async status(cmd: number, body?: Uint8Array, version?: number): Promise<number> {
    return (await this.request(cmd, body, version)).status;
  }
  /** One request that must be answered with status 0. */
  async send(cmd: number, body?: Uint8Array): Promise<void> {
    const status = await this.status(cmd, body);
    if (status !== 0) throw new Error(`request ${cmd.toString(16)} answered ${status}`);
  }

  /** Close the daemon, then check the write-watch of every boot and the card sectors of every overlay. */
  async finish(): Promise<void> {
    try {
      await this.leave();
    } finally {
      await this.session.close();
    }
    if (!this.overlays.length) return;
    const report = runtimeSafety(this.device, this.watchOutput, this.overlays, this.epochs.size);
    record(this.dir, 'upload-safety', report);
    expect(report.write_events, 'write-watch on cf.expansion over every boot: no stray write').toBe(0);
    expect(report.emulators).toBeGreaterThan(0);
    expect(report.flush_hits, 'flush check over every boot: no hit').toBe(0);
    const { first, end, firmwareFirst } = this.store;
    const written = new Set(this.overlays.flatMap((f) => Array.from(overlayRecords(fs.readFileSync(f)), (r) => r.sector)));
    const outside = [...written].filter((s) => !(first <= s && s < end) && s < firmwareFirst).sort((a, b) => a - b);
    expect(
      outside.map((s) => `0x${s.toString(16)}`),
      'card writes: the module store range and the firmware project sectors',
    ).toEqual([]);
  }
}

/** One test per device on its own reusable runtime daemon. `symbols`: runtime variables the test reads or sets (`DKRT_BOOT` is always planned). */
export function uploadTests(
  name: string,
  body: (rig: UploadRig) => Promise<void>,
  opts: { devices?: readonly Device[]; full?: boolean; symbols?: string[]; sharc?: boolean; timeout?: number } = {},
): void {
  for (const device of opts.devices ?? DEVICES) {
    describe.skipIf(opts.full === true && !FULL_TIER)(`${device.toUpperCase()} ${FIRMWARE[device]}`, () => {
      let rig: UploadRig | undefined;
      beforeAll(async () => {
        rig = await UploadRig.start(device, name.split(':')[0]!, opts.symbols ?? [], opts.sharc ? 'sharc' : CF_ONLY_DSP);
      }, 240_000);
      afterAll(async () => {
        await rig?.finish();
      }, 240_000);
      test(
        name,
        async () => {
          await body(rig!);
        },
        opts.timeout ?? 1_800_000,
      );
    });
  }
}

// ---- the +Drive round trip ----------------------------------------------------------------------------------------------------------

/** `plusdrive export` of the card with the guest's writes writes the store's files to `digikit/` of a folder; `plusdrive build` of that folder gives a card whose store holds the same files. */
export async function roundtrip(rig: UploadRig, files: Files): Promise<void> {
  const overlay = await rig.overlay('roundtrip');
  const top = path.join(rig.dir, 'roundtrip');
  const folder = path.join(top, 'plus_drive', rig.device);
  fs.rmSync(top, { recursive: true, force: true });
  fs.mkdirSync(folder, { recursive: true });
  const exportArgs = ['export', rig.image, '--overlay', overlay, '-o', folder, '--out', path.join(top, 'out'), '--device', rig.device];
  const first = await run(plusdriveBinary(), exportArgs);
  const digikit = path.join(folder, 'digikit');
  const got = fs.existsSync(digikit) ? walk(digikit) : {};
  delete got['selection.txt'];
  expect(first.code, `+Drive round trip: the export: ${first.stderr.slice(-200)}`).toBe(0);
  expect(listing(got), '+Drive round trip: the export wrote the store files to digikit/').toEqual(listing(files));
  const second = await run(plusdriveBinary(), exportArgs);
  expect(second.code, second.stderr.slice(-200)).toBe(0);
  expect(second.stdout, '+Drive round trip: a second export adds, replaces and keeps nothing').toContain('added 0 replaced 0');
  expect(second.stdout).toContain('kept 0');
  const built = path.join(top, 'card.img');
  const syx = resolveInputs(rig.device, FIRMWARE[rig.device]).syx;
  const build = await run(plusdriveBinary(), ['build', folder, '-o', built, '--device', rig.device, '--syx', syx]);
  expect(build.code, build.stderr.slice(-200)).toBe(0);
  const names = (await storeList(built, rig.device)).entries.map((e) => e.path).sort();
  expect(names, '+Drive round trip: the next card build holds the uploaded files').toEqual(Object.keys(files).sort());
}
