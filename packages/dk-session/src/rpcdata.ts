// The stock MidiRpc Data API for sample slots (finding 142 section 4): List, ReadOpen/Partial/Close, WriteOpen/Partial/Close and Clear on the route `/samples`.
// A client over `RpcPort` (the transport of `DkSession.rpc`). It takes a slot NUMBER and builds the path itself, so no other route (`/projects`, `/soundbanks`)
// can be named. Every reply starts with a status byte: 1 OK, 0 an error followed by its NUL ended text. Integers big-endian.
// Written without TS-only runtime syntax so node can import it.
import { DkError, be32, concat, u32 } from './dkupload-core.ts';
import {
  BACKUP_FOOTER,
  BACKUP_HEADER,
  SLOT_IMAGE_MAX,
  SLOT_COUNT,
  checkSlot,
  fwCrc,
  unwrapBackup,
} from './backup-file.ts';
import { RPC_CHUNK } from './rpcfiles.ts';
import type { RpcPort } from './rpcfiles.ts';

export const DATA = {
  LIST: 0x53,
  READ_OPEN: 0x54,
  READ_PARTIAL: 0x55,
  READ_CLOSE: 0x56,
  WRITE_OPEN: 0x57,
  WRITE_PARTIAL: 0x58,
  WRITE_CLOSE: 0x59,
  CLEAR: 0x5c,
} as const;
/** The route of the sample slots. */
export const SLOT_ROUTE = '/samples';
/** The largest data length of one ReadPartial the firmware allows. */
export const READ_CHUNK = 0x4000;
const OK = 1;
const text = new TextEncoder();
const textIn = new TextDecoder();

/** One slot of a List: `used` is false for an empty slot; `size` is the slot image size (0x40 + audio), 0 when empty. */
export interface SlotRow {
  slot: number;
  name: string;
  size: number;
  used: boolean;
}

const cpath = (path: string) => concat(text.encode(path), Uint8Array.of(0));
const slotPath = (slot: number) => {
  checkSlot(slot);
  return cpath(`${SLOT_ROUTE}/${slot}`);
};
const cancelled = () => new DkError({ error: 'cancelled', kind: 'cancelled' });
const protocol = (error: string) => new DkError({ error, kind: 'protocol' });

export class RpcData {
  private port: RpcPort;
  private chunk: number;
  private timeoutMs?: number;
  constructor(port: RpcPort, options: { chunk?: number; timeoutMs?: number } = {}) {
    this.port = port;
    // the first WritePartial must hold the 31-byte header
    this.chunk = Math.max(0x40, options.chunk ?? port.rpcChunk ?? RPC_CHUNK);
    this.timeoutMs = options.timeoutMs;
  }

  /** One request; the reply after its status byte. Status 0 is the device's error, with its text. */
  private async call(type: number, what: string, payload: Uint8Array): Promise<Uint8Array> {
    const r = await this.port.rpc(type, payload, this.timeoutMs);
    if (r.length === 0) throw protocol(`${what}: empty reply`);
    if (r[0] !== OK) {
      const end = r.indexOf(0, 1);
      const why = textIn.decode(r.subarray(1, end < 0 ? r.length : end));
      throw new DkError({ error: `${what}: ${why || `status ${r[0]}`}`, kind: 'device', status: `DATA_${r[0]}` });
    }
    return r.subarray(1);
  }
  private need(b: Uint8Array, n: number, what: string) {
    if (b.length < n) throw protocol(`${what}: a ${b.length}-byte reply, ${n} expected`);
  }

  /** All slots (first index 0, end 0, `exist_only` 0). A slot the reply does not list is not in the result. */
  async list(): Promise<SlotRow[]> {
    const b = await this.call(DATA.LIST, 'list', concat(cpath(SLOT_ROUTE), be32(0), be32(0), Uint8Array.of(0)));
    this.need(b, 12, 'list');
    const count = u32(b, 8);
    if (count > SLOT_COUNT) throw protocol(`list: ${count} entries`);
    const rows: SlotRow[] = [];
    let at = 12;
    for (let i = 0; i < count; i++) {
      const end = b.indexOf(0, at);
      if (end < 0) throw protocol('list: an entry without NUL');
      const name = textIn.decode(b.subarray(at, end));
      at = end + 1;
      this.need(b, at + 14, 'list');
      // u8 0, u8 kind (2 = item), u32 slot, u32 size, u16 permissions, u8 exists, u8 0
      const slot = u32(b, at + 2);
      const size = u32(b, at + 6);
      const exists = b[at + 12] === 1;
      at += 14;
      if (slot < 1 || slot > SLOT_COUNT) throw protocol(`list: slot ${slot}`);
      rows.push({ slot, name, size, used: exists || size > 0 });
    }
    return rows;
  }

  async readOpen(slot: number, chunk = READ_CHUNK): Promise<{ job: number; chunk: number }> {
    const b = await this.call(
      DATA.READ_OPEN,
      `read slot ${slot}`,
      concat(slotPath(slot), be32(chunk), Uint8Array.of(0)),
    );
    this.need(b, 9, 'read open');
    if (b[8] !== 0) throw protocol('read open: the device compressed the stream');
    return { job: u32(b, 0), chunk: u32(b, 4) };
  }
  /** One chunk. `seq` starts at 1. The chunk CRC is checked. */
  async readPartial(job: number, seq: number): Promise<{ data: Uint8Array; complete: boolean }> {
    const b = await this.call(DATA.READ_PARTIAL, 'read', concat(be32(job), be32(seq)));
    this.need(b, 21, 'read');
    if (u32(b, 0) !== job || u32(b, 4) !== seq) throw protocol('read: the reply is for another job or chunk');
    const len = u32(b, 17);
    this.need(b, 21 + len, 'read');
    const data = b.slice(21, 21 + len);
    if (u32(b, 13) !== fwCrc(data)) throw protocol(`read: the CRC of chunk ${seq} does not match`);
    return { data, complete: b[12] === 1 };
  }
  async readClose(job: number): Promise<void> {
    await this.call(DATA.READ_CLOSE, 'read close', be32(job));
  }

  /** The whole backup file of a slot (open, chunks until the device says complete, close). */
  async readFile(slot: number, signal?: AbortSignal): Promise<Uint8Array> {
    const { job, chunk } = await this.readOpen(slot);
    const parts: Uint8Array[] = [];
    let total = 0;
    for (let seq = 1; ; seq++) {
      if (signal?.aborted) throw cancelled();
      const r = await this.readPartial(job, seq);
      parts.push(r.data);
      total += r.data.length;
      if (total > BACKUP_HEADER + SLOT_IMAGE_MAX + BACKUP_FOOTER) throw protocol('read: more data than one slot holds');
      if (r.complete) break;
      if (r.data.length === 0 || seq > (SLOT_IMAGE_MAX + 64) / Math.max(chunk, 0x10) + 4)
        throw protocol('read: the device does not finish');
    }
    await this.readClose(job);
    return concat(...parts);
  }

  /** `declared` is the length of the whole file (the firmware does not use it). */
  async writeOpen(slot: number, declared: number): Promise<number> {
    const b = await this.call(DATA.WRITE_OPEN, `write slot ${slot}`, concat(be32(declared), slotPath(slot)));
    this.need(b, 4, 'write open');
    return u32(b, 0);
  }
  /** One chunk; `seq` starts at 0. The data length is the real length (the firmware allocates it). */
  async writePartial(job: number, seq: number, data: Uint8Array): Promise<void> {
    const b = await this.call(
      DATA.WRITE_PARTIAL,
      'write',
      concat(be32(job), be32(seq), be32(fwCrc(data)), be32(data.length), data),
    );
    this.need(b, 12, 'write');
    if (u32(b, 0) !== job || u32(b, 4) !== seq) throw protocol('write: the reply is for another job or chunk');
  }
  async writeClose(job: number): Promise<void> {
    await this.call(DATA.WRITE_CLOSE, 'write close', concat(be32(job), be32(0)));
  }

  /**
   * Send a backup file into `slot`: exactly 31 + size + 12 bytes. Refuses first (before WriteOpen) a file that `unwrapBackup` does not accept (CRC, size over one
   * slot, wrong type) or that is for another slot than `slot`. `onProgress(sent, total)` counts file bytes. A failed or cancelled send leaves the slot empty (the firmware
   * writes the header sector last) and no close is sent: the next open request closes the job.
   */
  async writeFile(
    slot: number,
    file: Uint8Array,
    onProgress?: (sent: number, total: number) => void,
    signal?: AbortSignal,
  ): Promise<void> {
    const { slot: inFile, image } = unwrapBackup(file);
    if (inFile !== slot)
      throw new DkError({ error: `the file is for slot ${inFile}, not slot ${slot}`, kind: 'usage' });
    if (file.length !== BACKUP_HEADER + image.length + BACKUP_FOOTER)
      throw new DkError({ error: 'the file has bytes after its footer', kind: 'usage' });
    if (signal?.aborted) throw cancelled();
    const job = await this.writeOpen(slot, file.length);
    // the firmware's stream writer takes bytes past the body size (MmcStreamWriter at 0x400bbbe4, finding 142): the footer must come in its own chunk.
    // The body chunks end exactly at 31 + size; the first one holds the 31-byte header (the chunk is at least 0x40).
    const bodyEnd = file.length - BACKUP_FOOTER;
    let seq = 0;
    for (let at = 0; at < bodyEnd; at += this.chunk, seq++) {
      if (signal?.aborted) throw cancelled();
      const end = Math.min(bodyEnd, at + this.chunk);
      await this.writePartial(job, seq, file.subarray(at, end));
      onProgress?.(end, file.length);
    }
    if (signal?.aborted) throw cancelled();
    await this.writePartial(job, seq, file.subarray(bodyEnd));
    onProgress?.(file.length, file.length);
    await this.writeClose(job);
  }

  /** Erase the header sector of a slot: the slot is empty. */
  async clear(slot: number): Promise<void> {
    await this.call(DATA.CLEAR, `clear slot ${slot}`, slotPath(slot));
  }
}
