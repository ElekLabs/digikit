// A fake of the stock firmware's Data API for sample slots, behind `RpcPort`. It follows finding 142, including the dangerous parts: the write size is taken from the
// file header with no clamp, the first sector of a slot is written last, a bad header is refused only after the body of the first chunk was written. The card is 70 slots
// long so that a write past slot 64 is visible. It has its own CRC and export builder (not the code under test).
import type { RpcPort } from '../src/rpcfiles.ts';

export const SLOT = 0x80000;
const enc = new TextEncoder();
const ok = (...p: Uint8Array[]) => cat(Uint8Array.of(1), ...p);
const err = (why: string, extra: Uint8Array = new Uint8Array()) =>
  cat(Uint8Array.of(0), enc.encode(why), Uint8Array.of(0), extra);
const be = (n: number) => Uint8Array.of(n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255);
function cat(...p: Uint8Array[]) {
  const o = new Uint8Array(p.reduce((n, x) => n + x.length, 0));
  let at = 0;
  for (const x of p) {
    o.set(x, at);
    at += x.length;
  }
  return o;
}
const rd = (b: Uint8Array, at: number) => new DataView(b.buffer, b.byteOffset, b.length).getUint32(at);
/** The chunk CRC, bitwise: reflected 0xedb88320, register 0, xorout 0xffffffff. */
export function crcBitwise(b: Uint8Array, seed = 0xffffffff): number {
  let c = ~seed >>> 0;
  for (const x of b) {
    c ^= x;
    for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
  }
  return ~c >>> 0;
}

type Job =
  | { kind: 'read'; slot: number; chunk: number; data: Uint8Array; last: number; done: boolean }
  | {
      kind: 'write';
      slot: number;
      seq: number;
      head: number[];
      size: number;
      body: number;
      first: Uint8Array;
      crc: number;
      foot: number[];
      footDone: boolean;
      accepted: number;
      headerOk: boolean;
    };

export class FakeDataApi implements RpcPort {
  readonly card = new Uint8Array(70 * SLOT);
  /** Every request: type and payload. */
  readonly sent: { type: number; payload: Uint8Array }[] = [];
  /** Paths outside `/samples` that a request named (a client must never do that). */
  readonly foreign: string[] = [];
  rpcChunk?: number;
  private job = 0;
  open: (Job & { id: number }) | undefined;
  /** Throw from `rpc` after this many requests (a lost link); undefined: never. */
  dropAfter: number | undefined;

  off(slot: number) {
    return (slot - 1) * SLOT;
  }
  private len(slot: number) {
    const o = this.off(slot);
    const magic = rd(this.card, o) === 0x53414d50;
    const n = magic ? rd(this.card, o + 8) : 0;
    return n >= 1 && n <= 0x3ffe0 ? n : 0;
  }
  nameOf(slot: number) {
    const o = this.off(slot) + 0xc;
    let end = o;
    while (end < o + 16 && this.card[end] !== 0) end++;
    return new TextDecoder().decode(this.card.subarray(o, end));
  }
  used(slot: number) {
    return this.len(slot) > 0;
  }
  /** Put a slot image into the slot directly (test setup). */
  load(slot: number, image: Uint8Array) {
    this.card.set(image, this.off(slot));
  }
  slotBytes(slot: number) {
    return this.card.subarray(this.off(slot), this.off(slot) + SLOT);
  }
  private path(p: Uint8Array): [string, number] {
    const end = p.indexOf(0);
    return [new TextDecoder().decode(p.subarray(0, end)), end + 1];
  }
  private slotOf(path: string): number | undefined {
    const m = /^\/samples\/(\d+)$/.exec(path);
    if (!m) {
      if (path !== '/samples') this.foreign.push(path);
      return undefined;
    }
    const n = Number(m[1]);
    return n >= 1 && n <= 64 ? n : undefined;
  }
  private exportFile(slot: number) {
    const n = this.len(slot);
    const img = this.card.slice(this.off(slot), this.off(slot) + n * 2 + 0x40);
    const h = new Uint8Array(31);
    h.set([0xac, 0x11, 0xd3, 0x03, 2, 0, 8, 0, 0x0d, 0, 0, 0, 0, 0, 0, 0, 8, 0xff, 0xff, 0xff, 0xff], 0);
    h.set(be(slot), 0x15);
    h.set(be(img.length), 0x19);
    h[0x1e] = 0x0c;
    return cat(h, img, be(crcBitwise(img)), be(0), be(0xaaa1daaa));
  }

  async rpc(type: number, payload: Uint8Array = new Uint8Array()): Promise<Uint8Array> {
    this.sent.push({ type, payload: payload.slice() });
    if (this.dropAfter !== undefined && this.sent.length > this.dropAfter) throw new Error('link lost');
    switch (type) {
      case 0x53: {
        const [path, at] = this.path(payload);
        if (path !== '/samples') {
          this.foreign.push(path);
          return err('invalid sample slot');
        }
        const existOnly = payload[at + 8] === 1;
        const rows: Uint8Array[] = [];
        for (let s = 1; s <= 64; s++) {
          const used = this.used(s);
          if (existOnly && !used) continue;
          rows.push(
            cat(
              enc.encode(used ? this.nameOf(s) : ''),
              Uint8Array.of(0, 0, 2),
              be(s),
              be(used ? this.len(s) * 2 + 0x40 : 0),
              Uint8Array.of(0, 0x7e, used ? 1 : 0, 0),
            ),
          );
        }
        return ok(be(0), be(65), be(rows.length), ...rows);
      }
      case 0x54: {
        const [path, at] = this.path(payload);
        const slot = this.slotOf(path);
        if (slot === undefined) return err('invalid sample slot');
        if (!this.used(slot)) return err('Failed to read');
        if (payload[at + 4] !== 0) return err('compression is not modelled');
        const chunk = Math.min(0x4000, Math.max(0x10, rd(payload, at)));
        this.open = { id: ++this.job, kind: 'read', slot, chunk, data: this.exportFile(slot), last: 0, done: false };
        return ok(be(this.open.id), be(chunk), Uint8Array.of(0));
      }
      case 0x55: {
        const j = this.open;
        if (!j || j.kind !== 'read' || rd(payload, 0) !== j.id) return err('Invalid job id');
        const seq = rd(payload, 4);
        if (seq !== j.last + 1 && seq !== j.last) return err('Invalid sequence number');
        j.last = seq;
        const from = (seq - 1) * j.chunk;
        const data = j.data.subarray(from, from + j.chunk);
        j.done = from + j.chunk >= j.data.length;
        return ok(be(j.id), be(seq), be(0), Uint8Array.of(j.done ? 1 : 0), be(crcBitwise(data)), be(data.length), data);
      }
      case 0x56: {
        const j = this.open;
        if (!j || j.kind !== 'read' || rd(payload, 0) !== j.id) return err('Invalid job id');
        if (!j.done) return err('Reader did not complete');
        this.open = undefined;
        return ok(be(j.id), be(0));
      }
      case 0x57: {
        const [path] = this.path(payload.subarray(4));
        const slot = this.slotOf(path);
        if (slot === undefined) return err('invalid sample slot');
        this.open = {
          id: ++this.job,
          kind: 'write',
          slot,
          seq: 0,
          head: [],
          size: 0,
          body: 0,
          first: new Uint8Array(0x200),
          crc: 0xffffffff,
          foot: [],
          footDone: false,
          accepted: 0,
          headerOk: false,
        };
        return ok(be(this.open.id));
      }
      case 0x58: {
        const j = this.open;
        if (!j || j.kind !== 'write' || rd(payload, 0) !== j.id)
          return err('File transfer is not active', Uint8Array.of(1));
        const len = rd(payload, 12);
        const data = payload.subarray(16, 16 + len);
        if (data.length !== len) return err('Failed to write data', Uint8Array.of(1));
        if (rd(payload, 4) !== j.seq) return err('Invalid sequence number', Uint8Array.of(1));
        if (rd(payload, 8) !== crcBitwise(data))
          return err('Invalid package checksum, corrupted data', Uint8Array.of(1));
        if (j.seq === 0 && len < 0x1f) return err('Unsupported payload size, header', Uint8Array.of(1));
        // the firmware flaw (finding 142, MmcStreamWriter at 0x400bbbe4): bytes after the body end in the same WritePartial are taken as body, not as footer
        let bodyDoneHere = false;
        for (const b of data) {
          if (j.head.length < 31) {
            j.head.push(b);
            if (j.head.length === 31) {
              j.size = rd(Uint8Array.from(j.head), 0x19); // no clamp: the firmware takes the size from the header
              this.card.fill(0, this.off(j.slot), this.off(j.slot) + 0x200); // the first sector is erased at the first write
            }
          } else if (j.body < j.size) {
            if (j.body < 0x200) j.first[j.body] = b;
            else if (this.off(j.slot) + j.body < this.card.length) this.card[this.off(j.slot) + j.body] = b;
            j.crc = crcBitwise(Uint8Array.of(b), j.crc);
            j.body++;
            if (j.body === j.size) {
              this.card.set(j.first.subarray(0, Math.min(0x200, j.size)), this.off(j.slot)); // the header sector last
              bodyDoneHere = true;
            }
          } else if (bodyDoneHere) {
            // swallowed as body: the writer never completes, so the header sector is not kept
            this.card.fill(0, this.off(j.slot), this.off(j.slot) + 0x200);
          } else if (j.foot.length < 12) {
            j.foot.push(b);
            if (j.foot.length === 12) {
              j.footDone = true;
              if (rd(Uint8Array.from(j.foot), 0) !== j.crc) return err('Content hash mismatch', Uint8Array.of(1));
            }
          }
        }
        j.accepted += len;
        j.seq++;
        // the header checks run after the body of the first chunk is in
        if (j.head.length === 31 && !j.headerOk) {
          const h = Uint8Array.from(j.head);
          if (h[5] !== 0 || h[6] !== 8) return err('File is not compatible with this firmware', Uint8Array.of(1));
          if (rd(h, 0x0d) !== 8) return err('Invalid file type', Uint8Array.of(1));
          j.headerOk = true;
        }
        return ok(be(j.id), be(j.seq - 1), be(j.accepted));
      }
      case 0x59: {
        const j = this.open;
        if (!j || j.kind !== 'write' || rd(payload, 0) !== j.id) return err('Invalid job id');
        if (j.body !== j.size || j.head.length < 31) return err('Writer did not process all data');
        if (!j.footDone) return err('Footer was not processed');
        this.open = undefined;
        return ok(be(j.id), be(0));
      }
      case 0x5c: {
        const [path] = this.path(payload);
        const slot = this.slotOf(path);
        if (slot === undefined) return err('invalid sample slot');
        this.card.fill(0, this.off(slot), this.off(slot) + 0x200); // the header sector only
        return ok();
      }
      default:
        return err('unsupported');
    }
  }
}
