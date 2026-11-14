// A fake Digitakt II that answers the stock MidiRpc FsSample commands byte for byte (docs/MIDI-SYSEX-RPC.md section 12), as a `DkTransport` for `DkSession`.
// Shared by the rpcfiles and samples tests.
import assert from 'node:assert/strict';
import { decode8in7, encode8in7, type DkTransport } from '@digi/dk-session';
import { RPC_FILE, TRAILER } from '@digi/dk-session/rpcfiles';

export const concat = (...p: ArrayLike<number>[]): Uint8Array =>
  Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
export const be32 = (n: number): Uint8Array =>
  Uint8Array.of((n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255);
export const u32 = (b: Uint8Array, at: number): number => new DataView(b.buffer, b.byteOffset + at, 4).getUint32(0);
const cstr = (b: Uint8Array, at: number): [string, number] => {
  const end = b.indexOf(0, at);
  return [Buffer.from(b.subarray(at, end)).toString(), end + 1];
};
/** A stand-in for the +Drive hash: FNV-1a over the bytes, bit 0 set (the client takes the function as an argument). */
export const hashOf = (b: Uint8Array): number => {
  let h = 0x811c9dc5;
  for (const x of b) h = Math.imul(h ^ x, 0x01000193) >>> 0;
  return (h | 1) >>> 0;
};

export class FakeDrive implements DkTransport {
  dirs = new Set(['/', '/factory']);
  files = new Map<string, Uint8Array>(); // path -> stored bytes (data + trailer)
  writer: { handle: number; name: string; data: Uint8Array[]; pos: number } | null = null;
  next = 6;
  queue: Uint8Array[] = [];
  seen: number[] = [];
  spaceLen: number;
  corrupt: boolean;
  /** Data bytes per write request this link takes (a test sets it; absent: a device link). */
  rpcChunk?: number;
  constructor({ spaceLen = 17, corrupt = false } = {}) {
    this.spaceLen = spaceLen;
    this.corrupt = corrupt;
  }
  reply(seq: Uint8Array, type: number, payload: Uint8Array): void {
    const body = concat(seq, seq, Uint8Array.of(type | 0x80), payload);
    this.queue.push(concat(Uint8Array.of(0xf0, 0x00, 0x20, 0x3c, 0x10, 0x00), encode8in7(body), Uint8Array.of(0xf7)));
  }
  send(msg: Uint8Array): void {
    assert.deepEqual([...msg.subarray(0, 6)], [0xf0, 0x00, 0x20, 0x3c, 0x10, 0x00]);
    const body = decode8in7(msg.subarray(6, msg.length - 1));
    assert.deepEqual([...body.subarray(2, 4)], [0, 0]);
    const seq = body.subarray(0, 2);
    const type = body[4];
    const p = body.subarray(5);
    this.seen.push(type);
    const ok = (...x: ArrayLike<number>[]) => this.reply(seq, type, concat(Uint8Array.of(1), ...x));
    const no = () => this.reply(seq, type, Uint8Array.of(0));
    switch (type) {
      case RPC_FILE.STORAGE_SPACE: {
        const nums = concat(new Uint8Array(4), be32(1000), new Uint8Array(4), be32(5000));
        // 17 bytes: status, free, total; 20 bytes: three more before them
        return this.reply(
          seq,
          type,
          this.spaceLen === 20 ? concat(Uint8Array.of(0, 0, 0, 1), nums) : concat(Uint8Array.of(1), nums),
        );
      }
      case RPC_FILE.READ_DIR: {
        const [dir] = cstr(p, 0);
        const parts = [];
        const under = (path: string) =>
          path !== dir && path.slice(0, path.lastIndexOf('/') || 1) === (dir === '/' ? '/' : dir);
        for (const d of this.dirs)
          if (under(d))
            parts.push(
              concat(
                be32(0),
                be32(0),
                Uint8Array.of(0, 'D'.charCodeAt(0)),
                Buffer.from(d.slice(d.lastIndexOf('/') + 1) + '\0'),
              ),
            );
        for (const [f, bytes] of this.files)
          if (under(f))
            parts.push(
              concat(
                be32(hashOf(bytes)),
                be32(bytes.length - 16),
                Uint8Array.of(0, 'F'.charCodeAt(0)),
                Buffer.from(f.slice(f.lastIndexOf('/') + 1) + '\0'),
              ),
            );
        return this.reply(seq, type, concat(...parts, new Uint8Array(0)));
      }
      case RPC_FILE.CREATE_DIR: {
        const [path] = cstr(p, 0);
        const parts = path.split('/').filter(Boolean);
        for (let i = 1; i <= parts.length; i++) this.dirs.add('/' + parts.slice(0, i).join('/'));
        return ok();
      }
      case RPC_FILE.DELETE_DIR: {
        const [path] = cstr(p, 0);
        const busy = [...this.files.keys(), ...this.dirs].some((x) => x.startsWith(path + '/'));
        if (!this.dirs.has(path) || busy) return no();
        this.dirs.delete(path);
        return ok();
      }
      case RPC_FILE.DELETE_FILE: {
        const [path] = cstr(p, 0);
        return this.files.delete(path) ? ok() : no();
      }
      case RPC_FILE.RENAME_FILE: {
        const [from, at] = cstr(p, 0);
        const [to] = cstr(p, at);
        if (!this.files.has(from)) return no();
        this.files.set(to, this.files.get(from) as Uint8Array);
        this.files.delete(from);
        return ok();
      }
      case RPC_FILE.FILE_INFO_V2: {
        const [path] = cstr(p, 0);
        const b = this.files.get(path);
        return this.reply(
          seq,
          type,
          b
            ? concat(Uint8Array.of(1), be32(b.length - 16), be32(hashOf(b)))
            : concat(Uint8Array.of(0), new Uint8Array(8)),
        );
      }
      case RPC_FILE.OPEN_WRITE: {
        const [name] = cstr(p, 4);
        // a bare name is refused (and makes a folder of its name), as the firmware does; so does a second writer
        if (!name.startsWith('/') || this.writer || !this.dirs.has(name.slice(0, name.lastIndexOf('/')) || '/')) {
          if (!name.startsWith('/')) this.dirs.add('/' + name);
          return no();
        }
        this.writer = { handle: this.next++, name, data: [], pos: 0 };
        return ok(be32(this.writer.handle));
      }
      case RPC_FILE.WRITE_V1: {
        const w = this.writer;
        const [handle, len, off] = [u32(p, 0), u32(p, 4), u32(p, 8)];
        if (!w || w.handle !== handle) return this.reply(seq, type, concat(Uint8Array.of(0), be32(0)));
        if (off !== w.pos) return this.reply(seq, type, concat(Uint8Array.of(0), be32(w.pos)));
        w.data.push(Uint8Array.from(p.subarray(12, 12 + len)));
        w.pos += len;
        return ok(be32(w.pos));
      }
      case RPC_FILE.CLOSE_WRITE: {
        const w = this.writer;
        // the byte count is required: a close with the handle alone fails
        if (!w || p.length < 8 || u32(p, 0) !== w.handle || u32(p, 4) !== w.pos)
          return this.reply(seq, type, concat(Uint8Array.of(0), new Uint8Array(8)));
        const stored = concat(...w.data, TRAILER);
        if (this.corrupt) stored[0] ^= 1;
        this.files.set(w.name, stored);
        this.writer = null;
        return ok(be32(w.handle), be32(w.pos));
      }
      default:
        return no();
    }
  }
  async recv(_timeoutMs?: number): Promise<Uint8Array | null> {
    return this.queue.shift() ?? null;
  }
}
