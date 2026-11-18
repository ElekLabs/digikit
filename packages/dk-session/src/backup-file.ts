// The backup file of the stock Data API (finding 142 section 4): what WriteOpen/WritePartial carry and ReadPartial returns for one sample slot.
// 31-byte header, the slot image, a 12-byte footer with the CRC of the image. All integers big-endian. LZ4 is never used (compress flag 0).
// Written without TS-only runtime syntax so node can import it.
import { DkError, be32, concat, u32 } from './dkupload-core.ts';

export const BACKUP_HEADER = 31;
export const BACKUP_FOOTER = 12;
const MAGIC = 0xac11d303;
const FOOTER_TAIL = 0xaaa1daaa;
const TYPE_SAMPLE = 8;
/** The sample slots of the route `/samples/N` (N = 1..SLOT_COUNT) and the bytes of one slot: the firmware does not clamp the size of a write, so a larger image would write into the next slots (finding 142 section 6). */
export const SLOT_COUNT = 64;
export const SLOT_IMAGE_MAX = 0x80000;
/** The slot header: magic `SAMP`, +4 unused, the length in samples at +8, the name at +0xC. */
export const SLOT_HEADER = 0x40;

const TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

/**
 * The firmware's CRC (chunk CRC and content hash, finding 142 "The chunk CRC"): the common CRC-32 (reflected polynomial 0xedb88320, xorout 0xffffffff) with the
 * register started at 0, i.e. seed 0xffffffff. CRC of "123456789" is 0xd202d277. Chaining: `fwCrc(b, fwCrc(a))` is the CRC of `a` then `b`.
 */
export function fwCrc(data: Uint8Array, seed = 0xffffffff): number {
  let c = ~seed >>> 0;
  for (const b of data) c = (TABLE[(c ^ b) & 0xff]! ^ (c >>> 8)) >>> 0;
  return ~c >>> 0;
}

const usage = (error: string) => new DkError({ error, kind: 'usage' });
const protocol = (error: string) => new DkError({ error, kind: 'protocol' });

export function checkSlot(slot: number): void {
  if (!Number.isInteger(slot) || slot < 1 || slot > SLOT_COUNT)
    throw usage(`sample slot ${slot}: slots are 1 to ${SLOT_COUNT}`);
}

/**
 * The file that goes over WriteOpen/WritePartial for `image` (the slot bytes: header and audio) into `slot`. Refuses before anything exists to send: a slot
 * outside 1..64, an image over one slot (0x80000), or one whose header is not `SAMP` with the length word `(size - 0x40) / 2` (the firmware reads that word at boot).
 * The result is exactly 31 + size + 12 bytes.
 */
export function wrapBackup(slot: number, image: Uint8Array): Uint8Array {
  checkSlot(slot);
  if (image.length > SLOT_IMAGE_MAX)
    throw usage(`the sample is ${image.length} bytes: a slot holds at most ${SLOT_IMAGE_MAX} (header included)`);
  if (image.length < SLOT_HEADER + 2 || (image.length - SLOT_HEADER) % 2 !== 0)
    throw usage(`not a slot image: ${image.length} bytes`);
  if (u32(image, 0) !== 0x53414d50 || u32(image, 8) !== (image.length - SLOT_HEADER) / 2)
    throw usage('not a slot image: the header is not SAMP with a length word that matches the size');
  const head = new Uint8Array(BACKUP_HEADER);
  const v = new DataView(head.buffer);
  v.setUint32(0, MAGIC);
  head[4] = 2;
  v.setUint16(5, 8);
  v.setUint16(7, 0x0d);
  v.setUint32(9, 0); // a word of the firmware version record; not checked by the importer
  v.setUint32(0x0d, TYPE_SAMPLE);
  v.setUint32(0x11, 0xffffffff);
  v.setUint32(0x15, slot);
  v.setUint32(0x19, image.length);
  head[0x1d] = 0; // not compressed
  head[0x1e] = BACKUP_FOOTER;
  return concat(head, image, be32(fwCrc(image)), be32(0), be32(FOOTER_TAIL));
}

/** The slot and the image of a backup file, after the checks the importer makes and the CRC of the footer. Bytes after the footer are ignored (as the firmware does). */
export function unwrapBackup(file: Uint8Array): { slot: number; image: Uint8Array } {
  if (file.length < BACKUP_HEADER + BACKUP_FOOTER)
    throw protocol(`backup file: ${file.length} bytes is shorter than its header and footer`);
  const v = new DataView(file.buffer, file.byteOffset, file.length);
  if (v.getUint32(0) !== MAGIC) throw protocol('backup file: wrong magic');
  if (v.getUint16(5) !== 8) throw protocol('backup file: not a file this firmware takes');
  if (v.getUint32(0x0d) !== TYPE_SAMPLE) throw protocol('backup file: not a sample');
  if (file[0x1d] !== 0) throw protocol('backup file: compressed (LZ4 is not supported)');
  const slot = v.getUint32(0x15);
  checkSlot(slot);
  const size = v.getUint32(0x19);
  if (size > SLOT_IMAGE_MAX) throw protocol(`backup file: image of ${size} bytes is over one slot`);
  if (file.length < BACKUP_HEADER + size + BACKUP_FOOTER) throw protocol('backup file: truncated');
  const image = file.subarray(BACKUP_HEADER, BACKUP_HEADER + size);
  if (v.getUint32(BACKUP_HEADER + size) !== fwCrc(image))
    throw protocol('backup file: the CRC of the image does not match');
  return { slot, image: image.slice() };
}
