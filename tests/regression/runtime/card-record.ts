import fs from 'node:fs';
import zlib from 'node:zlib';
import { type Device, type Session } from '@digi/test-support';
import { cardStore } from './device-data.ts';
import { rpc } from './guest-access.ts';
import { layOver } from './write-safety.ts';

/**
 * The selection record of a card: encoding, decoding and reading the two copies. Layout: packages/emulator/plusdrive/src/selection.rs.
 * The plusdrive CLI writes only seq 1, counter 0, no flags (selection.txt); a test needs other values and torn copies.
 */

export const SECTOR = 512;
const SEL_MAGIC = 0x444b534c;
export const SEL_MAX = 8;
const SEL_PATH = 56;

// ---- the selection record (one 512-byte copy) --------------------------------------------------------------------------------------

export interface SelRecord {
  paths: string[];
  seq: number;
  counter: number;
  flags: number;
  /** Stable period of safe mode in DSP blocks (0: the default). */
  stableBlocks: number;
  /** The 8 sticky machine-type entries (`hash24 << 8 | type`, 0 is empty). */
  map: number[];
}

/** Byte offset of sticky-map entry `i`. */
const mapAt = (i: number) => (i === 0 ? 28 : 480 + 4 * (i - 1));

export function encodeRecord({
  paths,
  seq = 1,
  counter = 0,
  flags = 0,
  stableBlocks = 0,
  map = [],
}: Partial<SelRecord>): Buffer {
  if (!paths || paths.length > SEL_MAX || paths.some((p) => !p || Buffer.byteLength(p) >= SEL_PATH))
    throw new Error('selection: at most 8 paths of 1 to 55 characters');
  const b = Buffer.alloc(SECTOR);
  [SEL_MAGIC, 1, seq, counter, flags, paths.length, stableBlocks].forEach((v, i) => b.writeUInt32BE(v >>> 0, 4 * i));
  paths.forEach((p, i) => b.write(p, 32 + i * SEL_PATH));
  map.forEach((e, i) => b.writeUInt32BE(e >>> 0, mapAt(i)));
  b.writeUInt32BE(zlib.crc32(b.subarray(0, 508)), 508);
  return b;
}

/** A valid record, or null: magic, version, CRC, count and NUL-terminated paths (the runtime's rules). */
export function decodeRecord(b: Buffer): SelRecord | null {
  if (b.length !== SECTOR || b.readUInt32BE(0) !== SEL_MAGIC || b.readUInt32BE(4) !== 1) return null;
  if (b.readUInt32BE(508) !== zlib.crc32(b.subarray(0, 508))) return null;
  const count = b.readUInt32BE(20);
  if (count > SEL_MAX) return null;
  const paths: string[] = [];
  for (let i = 0; i < count; i++) {
    const raw = b.subarray(32 + i * SEL_PATH, 32 + (i + 1) * SEL_PATH);
    if (raw[0] === 0 || !raw.includes(0)) return null;
    paths.push(raw.subarray(0, raw.indexOf(0)).toString());
  }
  return {
    paths,
    seq: b.readUInt32BE(8),
    counter: b.readUInt32BE(12),
    flags: b.readUInt32BE(16),
    stableBlocks: b.readUInt32BE(24),
    map: Array.from({ length: 8 }, (_, i) => b.readUInt32BE(mapAt(i))),
  };
}

/**
 * `count` sectors at `first` of the card the guest sees: the image (sparse and about 25 GB: read by position, never whole; bytes past
 * its end read as zeros), the `overlay.bin` next to it, then the optional `overlay` blob of a session's writes.
 */
export function readSectors(file: string, first: number, count: number, overlay?: Buffer): Buffer {
  const buf = Buffer.alloc(count * SECTOR);
  const fd = fs.openSync(file, 'r');
  try {
    fs.readSync(fd, buf, 0, buf.length, first * SECTOR);
  } finally {
    fs.closeSync(fd);
  }
  layOver(buf, first, file, overlay);
  return buf;
}

/** The record a boot uses: the valid copy with the higher seq (A when equal). */
export function currentRecord([a, b]: (SelRecord | null)[]): SelRecord | null {
  return a == null || (b != null && b.seq > a.seq) ? (b ?? null) : a;
}

/** The two copies of a card, with the guest's `card.overlay` bytes (optional) laid over it; a card that ends before the area reads as zeros. */
export function recordsIn(cardFile: string, device: Device, overlay?: Buffer): (SelRecord | null)[] {
  const { selA, selB } = cardStore(device);
  return [selA, selB].map((s) => decodeRecord(readSectors(cardFile, s, 1, overlay)));
}

/** The header sector the claim writes: an empty store, seq 1 (card.rs `claim`; earlier source: pk_digikit_debug.py `run_card_torn`). */
export function claimHeader(device: Device): Buffer {
  const { first, end } = cardStore(device);
  const b = Buffer.alloc(SECTOR);
  b.write('DKST', 0, 'latin1');
  [4, 0, 0, 1, 0, 1, 1, end - first].forEach((v, i) => b.writeUInt32BE(v >>> 0, 4 + 4 * i));
  b.writeUInt32BE(zlib.crc32(b.subarray(0, 508)), 508);
  return b;
}

/** The two record copies as the card holds them now: the daemon's overlay over the image. */
export async function selRecords(session: Session, cardImage: string, device: Device, overlayFile: string) {
  await rpc(session, 'card.overlay', { path: overlayFile });
  return recordsIn(cardImage, device, fs.readFileSync(overlayFile));
}
