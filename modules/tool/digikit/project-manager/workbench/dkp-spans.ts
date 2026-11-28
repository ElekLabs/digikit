/** The sections of a project file (`.dkp`, docs/design/digikit-project.md section 4) as coloured byte ranges, for the Raw view. The page reads the format only to colour it:
 * a file of another version, or without the magic, gets no spans and shows as plain hex. Ranges that run past the bytes (a file cut short) end at the last byte. */

import { valueView } from './project-text';

export type SpanKind =
  | 'magic'
  | 'version'
  | 'device'
  | 'slot'
  | 'name'
  | 'stamp'
  | 'counter'
  | 'used'
  | 'crc'
  | 'section'
  | 'module-id'
  | 'module-hash'
  | 'owner'
  | 'key'
  | 'value'
  | 'unknown';

/** Bytes `start` up to (not including) `end`. */
export interface Span {
  start: number;
  end: number;
  kind: SpanKind;
}

/** One decoded field of the file: `length` bytes at `offset`, its value as text, and the section it is listed under. `size` is the byte length of a saved value. */
export interface Field {
  label: string;
  offset: number;
  length: number;
  value: string;
  section: string;
  size?: number;
}

/** The checksum state: `ok` when the CRC-32 stored in the header matches the bytes (header without the CRC field, then the sections up to `used`). */
export type Checksum = 'ok' | 'mismatch';

export interface Decoded {
  spans: Span[];
  fields: Field[];
  checksum: Checksum;
}

export const DKP_VERSION = 1;
const HEADER = 42;
const TAG_MODULES = 1;
const TAG_DATA = 2;

const hex32 = (value: number) => value.toString(16).padStart(8, '0').toUpperCase();
const CRC_TABLE = Uint32Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
/** CRC-32 (zlib polynomial) over the given byte ranges in order. */
export function crc32(bytes: Uint8Array, ranges: [number, number][]): number {
  let crc = 0xffffffff;
  for (const [from, to] of ranges) for (let at = from; at < to; at++) crc = CRC_TABLE[(crc ^ bytes[at]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}
const textOf = (bytes: Uint8Array) => [...bytes].map((byte) => (byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : '?')).join('');

const u16 = (b: Uint8Array, at: number) => (b[at] << 8) | b[at + 1];
const u32 = (b: Uint8Array, at: number) => ((b[at] << 24) | (b[at + 1] << 16) | (b[at + 2] << 8) | b[at + 3]) >>> 0;

/** The spans of `bytes` in order and without gaps from 0, or undefined when the bytes are not a file of a version this page knows. */
export function dkpSpans(bytes: Uint8Array): Span[] | undefined {
  return decodeDkp(bytes)?.spans;
}

/** The spans, the decoded fields and the checksum state of `bytes`, or undefined when the bytes are not a file of a version this page knows. */
export function decodeDkp(bytes: Uint8Array): Decoded | undefined {
  if (bytes.length < HEADER || u32(bytes, 0) !== 0x444b504a || u16(bytes, 4) !== DKP_VERSION) return undefined;
  const spans: Span[] = [];
  const fields: Field[] = [];
  // No span reaches past `limit` (the file, then the section being read), so the spans stay in order even for a length field that lies.
  let limit = bytes.length;
  const put = (start: number, length: number, kind: SpanKind) => {
    const end = Math.min(start + length, limit);
    if (end > start) spans.push({ start, end, kind });
    return Math.max(end, start);
  };
  put(0, 4, 'magic');
  put(4, 2, 'version');
  put(6, 2, 'device');
  put(8, 1, 'slot');
  put(9, 17, 'name');
  put(26, 4, 'stamp');
  put(30, 4, 'counter');
  put(34, 4, 'used');
  put(38, 4, 'crc');
  const used = Math.min(u32(bytes, 34), bytes.length);
  const field = (label: string, offset: number, length: number, value: string, section: string, size?: number) => {
    const end = Math.min(offset + length, limit);
    if (end > offset) fields.push({ label, offset, length: end - offset, value, section, ...(size === undefined ? {} : { size }) });
  };
  const nameLength = Math.min(bytes[9], 16);
  field('magic', 0, 4, 'DKPJ', 'Header');
  field('version', 4, 2, String(u16(bytes, 4)), 'Header');
  field('device', 6, 2, String(u16(bytes, 6)), 'Header');
  field('slot', 8, 1, String(bytes[8]), 'Header');
  field('name', 9, 17, textOf(bytes.slice(10, 10 + nameLength)), 'Header');
  field('stamp', 26, 4, hex32(u32(bytes, 26)), 'Header');
  field('write counter', 30, 4, String(u32(bytes, 30)), 'Header');
  field('used', 34, 4, String(u32(bytes, 34)), 'Header');
  field('crc', 38, 4, hex32(u32(bytes, 38)), 'Header');
  const claimed = u32(bytes, 34);
  const checksum: Checksum =
    claimed >= HEADER && claimed <= bytes.length && crc32(bytes, [[0, 38], [HEADER, claimed]]) === u32(bytes, 38) ? 'ok' : 'mismatch';
  let at = HEADER;
  while (at + 3 <= used) {
    const tag = bytes[at];
    const end = Math.min(at + 3 + u16(bytes, at + 1), used);
    put(at, 3, 'section');
    let pos = at + 3;
    limit = end;
    if (tag === TAG_MODULES) {
      while (pos < end) {
        const length = bytes[pos];
        if (pos + 1 + length + 4 > end) break;
        field('module', pos, 1 + length, textOf(bytes.slice(pos + 1, pos + 1 + length)), 'Modules');
        pos = put(pos, 1 + length, 'module-id');
        field('hash', pos, 4, hex32(u32(bytes, pos)), 'Modules');
        pos = put(pos, 4, 'module-hash');
      }
    } else if (tag === TAG_DATA) {
      const ownerLength = bytes[pos] ?? 0;
      const owner = textOf(bytes.slice(pos + 1, Math.min(pos + 1 + ownerLength, end)));
      const section = `Settings ${owner}`;
      field('owner', pos, 1 + ownerLength, owner, section);
      pos = put(pos, 1 + ownerLength, 'owner');
      while (pos < end) {
        const keyLength = bytes[pos];
        if (pos + 1 + keyLength + 2 > end) break;
        const keyAt = pos;
        const key = textOf(bytes.slice(pos + 1, pos + 1 + keyLength));
        pos = put(pos, 1 + keyLength, 'key');
        const valueLength = u16(bytes, pos);
        if (pos + 2 + valueLength > end) break;
        const view = valueView(bytes.slice(pos + 2, pos + 2 + valueLength), valueLength);
        field(key, keyAt, pos + 2 + valueLength - keyAt, view.text, section, valueLength);
        pos = put(pos, 2 + valueLength, 'value');
      }
    }
    if (tag !== TAG_MODULES && tag !== TAG_DATA && end > pos) field(`tag ${tag}`, pos, end - pos, '', 'Unknown', end - pos);
    // what is left of the section (an unknown tag's body, or bytes a known body did not parse into)
    put(pos, end - pos, 'unknown');
    limit = bytes.length;
    at = end;
  }
  // the bytes after `used`, and any gap, are plain
  return { spans, fields, checksum };
}

/** The kind of the byte at `index`, or undefined for a byte no span covers. `spans` is in order (binary search). */
export function kindAt(spans: Span[], index: number): SpanKind | undefined {
  let low = 0;
  let high = spans.length - 1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    const span = spans[mid];
    if (index < span.start) high = mid - 1;
    else if (index >= span.end) low = mid + 1;
    else return span.kind;
  }
  return undefined;
}
