/** The sections of a project file (`.dkp`, docs/design/digikit-project.md section 4) as coloured byte ranges, for the Raw view. The page reads the format only to colour it:
 * a file of another version, or without the magic, gets no spans and shows as plain hex. Ranges that run past the bytes (a file cut short) end at the last byte. */

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

export const DKP_VERSION = 1;
const HEADER = 42;
const TAG_MODULES = 1;
const TAG_DATA = 2;

const u16 = (b: Uint8Array, at: number) => (b[at] << 8) | b[at + 1];
const u32 = (b: Uint8Array, at: number) => ((b[at] << 24) | (b[at + 1] << 16) | (b[at + 2] << 8) | b[at + 3]) >>> 0;

/** The spans of `bytes` in order and without gaps from 0, or undefined when the bytes are not a file of a version this page knows. */
export function dkpSpans(bytes: Uint8Array): Span[] | undefined {
  if (bytes.length < HEADER || u32(bytes, 0) !== 0x444b504a || u16(bytes, 4) !== DKP_VERSION) return undefined;
  const spans: Span[] = [];
  const put = (start: number, length: number, kind: SpanKind) => {
    const end = Math.min(start + length, bytes.length);
    if (end > start) spans.push({ start, end, kind });
    return start + length;
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
  let at = HEADER;
  while (at + 3 <= used) {
    const tag = bytes[at];
    const end = Math.min(at + 3 + u16(bytes, at + 1), used);
    put(at, 3, 'section');
    let pos = at + 3;
    if (tag === TAG_MODULES) {
      while (pos < end) {
        const length = bytes[pos];
        if (pos + 1 + length + 4 > end) break;
        pos = put(pos, 1 + length, 'module-id');
        pos = put(pos, 4, 'module-hash');
      }
    } else if (tag === TAG_DATA) {
      const owner = bytes[pos] ?? 0;
      pos = put(pos, 1 + owner, 'owner');
      while (pos < end) {
        const keyLength = bytes[pos];
        if (pos + 1 + keyLength + 2 > end) break;
        pos = put(pos, 1 + keyLength, 'key');
        const valueLength = u16(bytes, pos);
        if (pos + 2 + valueLength > end) break;
        pos = put(pos, 2 + valueLength, 'value');
      }
    }
    // what is left of the section (an unknown tag's body, or bytes a known body did not parse into)
    put(pos, end - pos, 'unknown');
    at = end;
  }
  // the bytes after `used`, and any gap, are plain
  return spans;
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
