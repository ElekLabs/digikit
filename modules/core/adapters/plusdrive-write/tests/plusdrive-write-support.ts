import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { be32, fileHash, type DkSession } from '@digi/dk-session';
import { repoRoot } from '@digi/test-support';
import { expect } from 'vitest';
import { overlayArgs, plusdriveBinary, readSectors } from '@digi/regression/runtime/card-kit.ts';
import { cardRigs, RT, type CardRig } from '@digi/regression/runtime/card-rig.ts';
import { word, SETTLE } from '@digi/regression/runtime/fx-live.ts';
import { overlayFile, overlayRecords } from '@digi/regression/runtime/write-safety.ts';

// Shared setup of the +Drive write device tests (plusdrive-write-{files,hash,levers,cuts}.device.test.ts): the helpers that read the card, and the rig
// context (seed card, job runner, snapshots, hash table and inode readers). Each file boots its own rig; the cases do not depend on each other.

/**
 * Writes to the stock +Drive through core/plusdrive-write (docs/plans/usb-fast.md 3.1, finding 120), on the Digitakt II 1.17. The fixture
 * test/file-write starts an I/O job that writes a file of a known pattern through file_* on DK_FILES_SAMPLES. The card is read back with
 * `plusdrive fs ls` / `fs cat` through the overlay of the guest's writes: the path filesystem as the firmware left it.
 */

export const exec = promisify(execFile);
export const CMD = 0x24;
export const E_EXISTS = -18;
export const E_FILE_IN_USE = -71;
export const WRITE = 1;
export const ABORT = 2;
export const DELETE = 6;
export const BIG_CALL = 10;
export const REPLACE = 0x40;
export const SAMPLES = 0x80 | 0x20; // the samples root; the expected hash covers the data and the 16 zero bytes
export const SAMPLES_ONLY = 0x80;
export const STAGING = '.dkstage';
export const pattern = (n: number) => Buffer.from(Uint8Array.from({ length: n }, (_, i) => (i * 7 + (i >> 8)) & 255));
export const cstr = (s: string) => Buffer.concat([Buffer.from(s), Buffer.from([0])]);
export const withTail = (b: Buffer) => Buffer.concat([b, Buffer.alloc(16)]);
export const slot4 = (b: Buffer) => Buffer.from(fileHash(withTail(b))).subarray(0, 4).toString('hex');

export interface Entry {
  size: number;
  hash: string;
}

export async function drive(card: string): Promise<Map<string, Entry>> {
  const { stdout } = await exec(plusdriveBinary(), ['fs', 'ls', card, ...overlayArgs(card)], { cwd: repoRoot(), maxBuffer: 1 << 26 });
  const out = new Map<string, Entry>();
  for (const line of stdout.trim().split('\n')) {
    const [p, size, hash] = line.split('\t') as [string, string, string];
    if (p.endsWith('/')) out.set(p, { size: 0, hash: '' });
    else out.set(p, { size: Number(size), hash });
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

/** Files that are not part of the written file's story: everything but the destination and the staging folder. */
export const others = (m: Map<string, Entry>, except: string[]) =>
  [...m].filter(([p]) => !except.includes(p) && !p.startsWith(`${STAGING}/`) && p !== `${STAGING}/`);
export const noStagingFolder = (m: Map<string, Entry>) => !m.has(`${STAGING}/`);
export const staged = (m: Map<string, Entry>) => [...m.keys()].filter((p) => p.startsWith(`${STAGING}/`) && p !== `${STAGING}/`);

export const FS_FIRST = 0x5d8000; // the superblock "SFke"; the store and the settings are below
export const HASH_PAGES = [0x5ee980, 0x5ee980 + 44 * 0x40]; // the 44 hash pages
export const TABLE = 0x4067d8c0; // the RAM hash table: count, then (hash, id) pairs
// The inode records of the path filesystem: 128-byte records, 256 per 32 KiB page, from sector 0x5d8180 (packages/emulator/plusdrive/src/reader.rs).
export const REC_FIRST = 0x5d8180;
export const REC_SECTORS = 0x16000;
export const REC = 0x80;
export const BITMAP = 0x46f39ca0;
export const FRAG_LAST_WORD = 0x5004; // the allocator scans this many words (FUN_4015583c)
// The runtime calls the adapter's recovery after 2 s without a write session (PD_STAGE_IDLE): the fast close rebuilds the RAM hash table then.
export const IDLE_MS = 4000;

export function makeCtx(rig: CardRig) {
  const SEEDS: Record<string, Buffer> = {
    'keep.bin': pattern(300),
    'Samples/old.bin': pattern(5000),
    'Samples/Sub/deep.bin': pattern(1234),
  };
  /** Let the boot recovery of the staging folder run. */
  const settle = async () => {
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
  };
  const bases = new Map<string, string>();
  const baseCard = async (extra: Record<string, Buffer> = {}, name = 'pd-base') => {
    const known = bases.get(name);
    if (known) return known;
    const mod = rig.mod('test/file-write');
    const drive_ = Object.entries({ ...SEEDS, ...extra }).map(([name, bytes]) => {
      const file = path.join(rig.artifacts, `seed-${name.replace(/[/.]/g, '_')}`);
      fs.writeFileSync(file, bytes);
      return { name, file };
    });
    const card = await rig.card({ name, modules: [mod], drive: drive_, selection: [mod.name] });
    bases.set(name, card);
    return card;
  };

  const start = async (dk: DkSession, kind: number, size: number, p: string) =>
    (await dk.request(CMD, Buffer.concat([Buffer.from([kind]), be32(size), cstr(p)]))).status;
  /** Run the emulator until the job ends; the job's fields. */
  const finish = async (dk: DkSession) => {
    for (let i = 0; i < 400; i++) {
      await rig.runtime.ui.wait({ ms: 100, why: SETTLE });
      const b = Buffer.from((await dk.request(CMD + 1)).body);
      if (b[0] === 2) return { create: b.readInt32BE(1), close: b.readInt32BE(5), abort: b.readInt32BE(9), job: b.readInt32BE(13), devHash: b.subarray(21, 25).toString('hex'), written: b.readUInt32BE(29) };
    }
    throw new Error('the job did not end');
  };
  const run = async (dk: DkSession, kind: number, size: number, p: string) => {
    expect(await start(dk, kind, size, p), 'the job is queued').toBe(0);
    return finish(dk);
  };
  const snapshot = async (tag: string) => {
    const card = await rig.after(tag);
    return { card, files: await drive(card) };
  };
  const same = (before: Map<string, Entry>, after: Map<string, Entry>, except: string[]) =>
    expect(others(after, except), 'no other file changed').toEqual(others(before, except));
  const fsWrites = (card: string) => {
    const m = new Map<number, string>();
    for (const r of overlayRecords(fs.readFileSync(overlayFile(card)))) if (r.sector >= FS_FIRST) m.set(r.sector, Buffer.concat([r.mask, r.data]).toString('hex'));
    return m;
  };
  const hashTable = async () => {
    const n = await word(rig.session, TABLE);
    const out: number[] = [n];
    for (let i = 0; i < n * 2; i++) out.push(await word(rig.session, TABLE + 4 + 4 * i));
    return out;
  };
  const inodes = (card: string) => readSectors(card, REC_FIRST, REC_SECTORS);
  const at = (area: Buffer, id: number) => area.subarray((Math.floor(id / 256) * 0x40) * 512 + (id % 256) * REC, (Math.floor(id / 256) * 0x40) * 512 + (id % 256) * REC + REC);
  const live = (area: Buffer, test: (r: Buffer) => boolean) => {
    const ids: number[] = [];
    for (let id = 2; id < (REC_SECTORS / 0x40) * 256; id++) {
      const r = at(area, id);
      if (r.readUInt16BE(2) > 0 && test(r)) ids.push(id);
    }
    return ids;
  };
  const fileOfSize = (area: Buffer, size: number) => live(area, (r) => (r[0]! & 1) === 0 && r.readUInt32BE(4) === size);
  const extentsOf = (area: Buffer, id: number) => {
    const r = at(area, id);
    const count = r.readUInt16BE(0x1e);
    const list: number[][] = [];
    if (count <= 8) for (let i = 0; i < count; i++) list.push([r.readUInt32BE(0x20 + 12 * i), r.readUInt32BE(0x24 + 12 * i), r.readUInt32BE(0x28 + 12 * i)]);
    return { count, list };
  };
  return { rig, settle, baseCard, start, finish, run, snapshot, same, fsWrites, hashTable, inodes, at, live, fileOfSize, extentsOf };
}

export type Pdw = ReturnType<typeof makeCtx>;

/** One describe per device with a rig and its context; `define` registers the file's cases. */
export function pdwSuite(label: string, o: { devices: ['dt2'] }, define: (t: Pdw) => void): void {
  cardRigs(
    label,
    { devices: o.devices, dsp: 'zero', roots: [...RT, 'core/plusdrive-write'], symbols: ['DKRT_MAILBOX', 'DKRT_SESSION_IDLE', 'DKRT_PD_CUT'], project: true, uploads: true, relaxFs: true },
    (rig: CardRig) => define(makeCtx(rig)),
  );
}
