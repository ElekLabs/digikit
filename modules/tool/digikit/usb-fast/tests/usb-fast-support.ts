import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { BULK_STATUS } from '@digi/dk-session/gen_bulk';
import {
  EmuBulkLink,
  EmuOutOnlyLink,
  chooseBulk,
  fileHash,
  type BulkChoice,
  type DkSession,
} from '@digi/dk-session';
import { repoRoot } from '@digi/test-support';
import { describe, expect } from 'vitest';
import { overlayArgs, plusdriveBinary } from '@digi/regression/runtime/card-store.ts';
import { cardRig } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { type CardRig } from '@digi/regression/runtime/card-rig-api.ts';
import { portFile } from '@digi/regression/runtime/device-data.ts';
import { DEVICES, FIRMWARE, FULL_TIER } from '@digi/regression/runtime/devices.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { rpc, word } from '@digi/regression/runtime/guest-access.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import type { Device } from '@digi/test-support';

/**
 * What the digikit/usb-fast device tests share (docs/plans/usb-fast-hardware.md, step 4): a card rig of the module, the two routes (the bulk pair and the OUT-only
 * interface), the destination folder of each device (the DN2 store, the DT2 +Drive) and the helpers that boot, negotiate the link and read the files back. The files:
 * usb-fast.device.test.ts (uploads), usb-fast-power-cut, usb-fast-mode, and usb-fast-bench (full tier). The emulator does not model the USB controller: what it cannot
 * show (enumeration, the real dTD engine, real throughput) is for the hardware run.
 */
export const exec = promisify(execFile);
export const SAMPLES = 1; // DK_FILES_SAMPLES: the store's samples/ folder on the DN2, the +Drive on the DT2
export const STORE = 2; // DK_FILES_STORE
export const E_EXISTS = BULK_STATUS.L_E_EXISTS;
export const STAGING = '.dkstage';
export const STEP = 100_000; // instructions an idle poll of the paused emulator steps (0.75 ms)
export const pattern = (n: number, seed = 0) =>
  Buffer.from(Uint8Array.from({ length: n }, (_, i) => (i * 7 + (i >> 8) + seed) & 255));
export const withTail = (b: Buffer) => Buffer.concat([b, Buffer.alloc(16)]);
export const slot = (b: Buffer, dt2: boolean) => Buffer.from(fileHash(dt2 ? withTail(b) : b));
/** The hash|1 as the u32 that END returns. */
export const hashU32 = (b: Buffer, dt2: boolean) => slot(b, dt2).readUInt32BE(0);
export const hex = (b: Buffer, dt2: boolean) => slot(b, dt2).toString('hex');

/**
 * The two routes of the core upload tests: the bulk pair of USB MIDI (port entry 2, replies on EP2 IN) and the OUT-only interface 5 of USB AUDIO/MIDI (port entry 5 with
 * core/usb-bulk-audio, replies over the SysEx 'R' poll). A mode picks the active port after the boot and the link class that talks to the guest.
 */
export interface Mode {
  label: string;
  port: number;
  /** INFO flags bit 0: replies over SysEx. */
  outOnly: boolean;
  callback: typeof EmuBulkLink.connectCallback;
  opener: typeof EmuBulkLink.opener;
}
export const PAIR: Mode = {
  label: 'bulk pair',
  port: 2,
  outOnly: false,
  callback: EmuBulkLink.connectCallback,
  opener: EmuBulkLink.opener,
};
export const MODES: Mode[] = [
  PAIR,
  {
    label: 'OUT-only',
    port: 5,
    outOnly: true,
    callback: EmuOutOnlyLink.connectCallback,
    opener: EmuOutOnlyLink.opener,
  },
];

export interface Entry {
  size: number;
  hash: string;
}

export async function drive(card: string): Promise<Map<string, Entry>> {
  const { stdout } = await exec(plusdriveBinary(), ['fs', 'ls', card, ...overlayArgs(card)], {
    cwd: repoRoot(),
    maxBuffer: 1 << 26,
  });
  const out = new Map<string, Entry>();
  for (const line of stdout.trim().split('\n')) {
    const [p, size, h] = line.split('\t') as [string, string, string];
    if (!p.endsWith('/')) out.set(p, { size: Number(size), hash: h });
  }
  return out;
}

export async function cat(card: string, p: string): Promise<Buffer> {
  const r = await exec(plusdriveBinary(), ['fs', 'cat', card, p, ...overlayArgs(card)], {
    cwd: repoRoot(),
    encoding: 'buffer',
    maxBuffer: 1 << 28,
  });
  return r.stdout;
}

/** STATUS 'S' [bit [0|1]]: the four speed switches (bit 0 hash once, bit 1 the USB receive ring, bit 2 the overlapped read-back hash, bit 3 large card commands); the reply is the mask now. */
export const speedMask = async (dk: DkSession, bit?: number, on?: number) =>
  Buffer.from(
    (await dk.request(0x37, Uint8Array.from(bit === undefined ? [0x53] : [0x53, bit, on!]))).body,
  ).readUInt32BE(0);

/** Everything the usb-fast device tests share: a card rig of the module, the destination folder of each device and the helpers that boot, connect and read the files. */
function build(device: Device) {
  const dt2 = device === 'dt2';
  const rig: CardRig = cardRig(device, 'usb-fast', {
    dsp: 'zero',
    roots: [...RT, 'core/usb-bulk', 'core/usb-bulk-audio', 'core/sdhc-large', ...(dt2 ? ['core/plusdrive-write'] : [])],
    symbols: [
      'DKRT_MAILBOX',
      'DKRT_SESSION_IDLE',
      'DKRT_UPLOAD_CUT',
      'DKRT_UPLOAD_TEAR',
      'DKRT_UPLOAD_WRITES',
      'DKRT_UPLOAD_FLIP',
      'usb_bulk_restarts',
      ...(dt2 ? ['DKRT_PD_CUT'] : []),
    ],
    project: true,
    uploads: true,
    relaxFs: true,
  });
  /** The path under the samples root: the +Drive's Samples folder is part of the path on the DT2, the store's samples/ folder is the root itself on the DN2. */
  const rp = (name: string) => (dt2 ? `Samples/${name}` : name);
  let connects = 0;
  const recovered = new WeakSet<object>();
  const call = <T>(method: string, params?: Record<string, unknown>) =>
    rig.session.client.call(method, params) as Promise<T>;
  const SEEDS: Record<string, Buffer> = { 'old.bin': pattern(5000, 3), 'keep.bin': pattern(1300, 5) };

  /** A card with usb-fast selected and two sample files; on the DT2 they are in the +Drive's Samples folder, on the DN2 in the store's samples/ folder. */
  const bases = new Map<string, string>();
  const baseCard = async (name = 'uf-base') => {
    const known = bases.get(name);
    if (known) return known;
    const mod = rig.mod('digikit/usb-fast');
    const files = Object.entries(SEEDS).map(([n, bytes]) => {
      const file = path.join(rig.artifacts, `seed-${n}`);
      fs.writeFileSync(file, bytes);
      return { name: dt2 ? `Samples/${n}` : `samples/${n}`, file };
    });
    const card = await rig.card(
      dt2
        ? { name, modules: [mod], drive: files, selection: [mod.name] }
        : { name, modules: [mod, ...files], selection: [mod.name] },
    );
    bases.set(name, card);
    return card;
  };

  /** The files of the destination folder as the device holds them now (name, size and hash), from the card after the guest's writes. */
  const state = async (dk: DkSession, tag: string): Promise<{ files: Map<string, Entry>; card?: string }> => {
    if (dt2) {
      const card = await rig.after(tag);
      const all = await drive(card);
      const files = new Map<string, Entry>();
      for (const [p, e] of all) if (p.startsWith('Samples/')) files.set(p.slice('Samples/'.length), e);
      for (const p of all.keys()) expect(p.startsWith(`${STAGING}/`), `no staging file left: ${p}`).toBe(false);
      return { files, card };
    }
    const files = new Map<string, Entry>();
    for (const e of await dk.list())
      if (e.path.startsWith('samples/')) files.set(e.path.slice('samples/'.length), { size: e.size, hash: e.hash });
    return { files };
  };
  /** The file is there with these bytes: size, hash, and the bytes themselves from the card (DT2) or by a read and VERIFY over SysEx (DN2). */
  const expectFile = async (
    dk: DkSession,
    snap: Awaited<ReturnType<typeof state>>,
    name: string,
    data: Buffer,
    what: string,
  ) => {
    const e = snap.files.get(name);
    expect(e, `${what}: ${name} is there`).toBeDefined();
    expect(e!.size, `${what}: size`).toBe(dt2 ? data.length + 16 : data.length);
    expect(e!.hash, `${what}: hash`).toBe(dt2 ? hex(data, true).slice(0, 8) : hex(data, false));
    if (dt2)
      expect((await cat(snap.card!, `Samples/${name}`)).subarray(0, data.length), `${what}: bytes`).toEqual(data);
    else {
      expect(await dk.verify(`samples/${name}`), `${what}: VERIFY reads the card`).toBe(hex(data, false));
      const mid = Math.floor(data.length / 2);
      expect(
        Buffer.from((await dk.read(`samples/${name}`, mid, Math.min(600, data.length - mid))).bytes),
        `${what}: bytes`,
      ).toEqual(data.subarray(mid, mid + Math.min(600, data.length - mid)));
    }
  };
  const unchanged = (before: Map<string, Entry>, after: Map<string, Entry>, except: string[]) => {
    const strip = (m: Map<string, Entry>) =>
      [...m].filter(([p]) => !except.includes(p)).sort(([a], [b]) => a.localeCompare(b));
    expect(strip(after), 'no other file changed').toEqual(strip(before));
  };

  /** Run until the module's job has enabled the interface (the guest's port table points at the adapter), then do what the host does: SET_NONCE over SysEx, find the device by its nonce. */
  const connect = async (
    dk: DkSession,
    options: { stepInstructions?: number; zlp?: boolean; timeoutMs?: number; chunk?: number; mode?: Mode } = {},
  ): Promise<BulkChoice> => {
    const mode = options.mode ?? PAIR;
    if (dt2 && !recovered.has(dk)) {
      recovered.add(dk);
      await rig.runtime.ui.wait({ ms: 2000, why: SETTLE }); // the boot recovery of the staging folder
    }
    for (let i = 0; i < 100 && (await mode.callback(call)) === undefined; i++)
      await rig.runtime.ui.wait({ ms: 100, why: SETTLE });
    const notes: string[] = [];
    const opener = mode.opener(call, { stepInstructions: options.stepInstructions ?? STEP, zlp: options.zlp });
    const choice = await chooseBulk(dk, opener, (l) => notes.push(l), {
      timeoutMs: options.timeoutMs ?? 20_000,
      resyncMs: 200,
      chunk: options.chunk,
      log: (l) => notes.push(l),
    });
    const dbg = choice
      ? ''
      : JSON.stringify(await rpc(rig.session, 'usb.bulk.out', {}).then((r) => (r as { status: unknown }).status)) +
        ' module ' +
        Buffer.from((await dk.request(0x37)).body).toString('hex');
    expect(
      choice,
      `${dbg} a bulk link is negotiated (${JSON.stringify(options)}, connect ${++connects}: ${notes.join('; ')})`,
    ).toBeDefined();
    return choice!;
  };
  /** The firmware's active USB port and its port table, by their anchors. */
  const usbAnchors = () => {
    const anchor = (name: string) => {
      const a = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((x) => x.name === name);
      expect(a.length, name).toBe(1);
      return a[0]!.addr as number;
    };
    return { port: anchor('cf.usb.value.active_port'), table: anchor('cf.usb.data.port_table') };
  };
  /** A menu-style change of the mode: the active port and the firmware's pointer to its table entry (the word below the port), then the bus reset of the new
   * enumeration (the connect callback of the new port's entry, when the guest has one), as the host sees a device that leaves and comes back. */
  const setPort = async (value: number) => {
    const a = usbAnchors();
    await rpc(rig.session, 'mem.cf.poke', { addr: a.port - 4, value: a.table + value * 0x28 });
    await rpc(rig.session, 'mem.cf.poke', { addr: a.port, value });
    if (value !== 2 && value !== 5) return;
    const connect = await (value === 5 ? EmuOutOnlyLink : EmuBulkLink).connectCallback(call);
    if (connect !== undefined) await rpc(rig.session, 'usb.bulk.in', { connect });
  };
  /** Boot, put the device in the mode's USB port (the boot is in USB MIDI) and negotiate the link. */
  const boot = async (mode: Mode = PAIR, name = 'uf-base') => {
    const dk = await rig.pausedBoot(await baseCard(name));
    let restarts = 0;
    if (mode.port !== 2) {
      await rig.runtime.ui.wait({ ms: 2500, why: SETTLE }); // the boot's enable first
      restarts = await word(rig.session, planSymbol(rig.plan, 'usb_bulk_restarts'));
      await setPort(mode.port);
    }
    const bulk = await connect(dk, { mode });
    // an OUT-only session starts with no stop and restart of the firmware's USB (the boot's enable in USB MIDI mode did its one)
    if (mode.outOnly)
      expect(
        await word(rig.session, planSymbol(rig.plan, 'usb_bulk_restarts')),
        'the OUT-only session does not restart the USB',
      ).toBe(restarts);
    return { dk, bulk };
  };
  const elapsed = async () => Number((await rig.session.status()).elapsed);
  return {
    device,
    dt2,
    rig,
    rp,
    call,
    baseCard,
    state,
    expectFile,
    unchanged,
    connect,
    usbAnchors,
    setPort,
    boot,
    elapsed,
  };
}
export type Harness = ReturnType<typeof build>;

/** A suite of the module's tests on each device: `register` adds the tests of one file. `full`: only in the full tier (`DIGIKIT_TIER=full`). */
export function ufSuite(name: string, register: (h: Harness) => void, options: { full?: boolean } = {}): void {
  for (const device of DEVICES) {
    describe.skipIf(options.full === true && !FULL_TIER)(
      `${device.toUpperCase()} ${FIRMWARE[device]} digikit/usb-fast ${name}`,
      () => register(build(device)),
    );
  }
}
