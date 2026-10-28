// A fake device's side of the RPC flash route (OsUpgradeStart 0x50, OsUpgradeWrite 0x51, OsUpgradeEnd 0x52; finding 75): checks the CRC of each Write,
// counts the bytes, answers as the emulated firmware does (replies are `counter16 seq16 (type | 0x80) payload`), and says when End asks for the flash.
// Shared by the screenshot runs of the Firmware page: tools/devtools/firmware_shots.mjs (site) and apps/desktop/ui/preview/firmware-mock.ts (desktop).
import { decode8in7, encode8in7 } from '@digi/dk-session';

const ENVELOPE = [0xf0, 0x00, 0x20, 0x3c, 0x10, 0x00];
export const RPC_START = 0x50;
export const RPC_WRITE = 0x51;
export const RPC_END = 0x52;

const TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
  return c >>> 0;
});
/** zlib's `crc32(data, 0xffffffff)`, the check the device makes. */
export function crc32(data) {
  let c = 0;
  for (const b of data) c = TABLE[(c ^ b) & 255] ^ (c >>> 8);
  return ~c >>> 0;
}

const u32 = (b, at) => ((b[at] << 24) | (b[at + 1] << 16) | (b[at + 2] << 8) | b[at + 3]) >>> 0;
const be32 = (n) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
const cstr = (s) => [...new TextEncoder().encode(s), 0];

export class FakeUpgrade {
  /**
   * `writeMs`: how long each Write takes to answer; `checkMs`: the last Write (the device checks the image); `errorAt`: a fraction of the image
   * at which a Write is answered with a device error (null: never).
   */
  constructor({ writeMs = 0, checkMs = 1200, errorAt = null } = {}) {
    this.writeMs = writeMs;
    this.checkMs = checkMs;
    this.errorAt = errorAt;
    this.counter = 1;
    this.total = 0;
    this.taken = 0;
    this.ends = [];
  }
  /** Is this message one of the three requests? */
  static is(msg) {
    if (msg[1] !== 0x00 || msg[2] !== 0x20) return false;
    const body = decode8in7(msg.subarray(6, msg.length - 1));
    return body[4] >= RPC_START && body[4] <= RPC_END;
  }
  reply(seq, ty, payload) {
    const body = Uint8Array.from([0, ++this.counter & 255, seq >> 8, seq & 255, ty | 0x80, ...payload]);
    return Uint8Array.from([...ENVELOPE, ...encode8in7(body), 0xf7]);
  }
  /** `{message, delayMs, commit}` for a request (`commit`: 1 or 0 for End), or null for a message that is not one of the three. */
  handle(msg) {
    const body = decode8in7(msg.subarray(6, msg.length - 1));
    const seq = (body[0] << 8) | body[1];
    const ty = body[4];
    const p = body.subarray(5);
    if (ty === RPC_START) {
      this.total = u32(p, 0);
      this.taken = 0;
      return { message: this.reply(seq, ty, [0, ...cstr('OK')]), delayMs: 40 };
    }
    if (ty === RPC_WRITE) {
      const crc = u32(p, 0);
      const n = u32(p, 4);
      const off = u32(p, 8);
      const data = p.subarray(12, 12 + n);
      if (crc !== crc32(data)) return { message: this.reply(seq, ty, [...be32(this.taken), 2, 0]), delayMs: 1 };
      if (this.errorAt !== null && off >= this.errorAt * this.total)
        return { message: this.reply(seq, ty, [...be32(off), 3, ...cstr('Checksum error!')]), delayMs: this.writeMs };
      if (off === this.taken) this.taken += n;
      const done = this.taken >= this.total;
      return {
        message: this.reply(seq, ty, [...be32(this.taken), done ? 1 : 0, 0]),
        delayMs: done ? this.checkMs : this.writeMs,
      };
    }
    if (ty === RPC_END) {
      const commit = p[0];
      this.ends.push(commit);
      return { message: this.reply(seq, ty, [commit ? 0 : 2, 0]), delayMs: 40, commit };
    }
    return null;
  }
}
