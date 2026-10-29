import { CMD } from '@digi/dk-session/gen_proto';
import type { DkSession } from '@digi/dk-session';

/** The value of `key` in a settings record (docs/design/digikit-store.md: `DKST`, u16 version, u32 seq, u32 used, u32 crc, then
 * `u8 key_len | key | u16 value_len | value` entries), or null. Not validated beyond the bounds: the runtime and `dksettings` check the CRC. */
export function settingsValue(record: Uint8Array, key: string): Uint8Array | null {
  const view = new DataView(record.buffer, record.byteOffset, record.byteLength);
  if (record.length < 18 || String.fromCharCode(...record.subarray(0, 4)) !== 'DKST') return null;
  const used = Math.min(view.getUint32(10), record.length);
  let at = 18;
  while (at < used) {
    const keyLen = record[at]!;
    const keyEnd = at + 1 + keyLen;
    if (keyLen === 0 || keyEnd + 2 > used) return null;
    const valueLen = view.getUint16(keyEnd);
    if (keyEnd + 2 + valueLen > used) return null;
    if (String.fromCharCode(...record.subarray(at + 1, keyEnd)) === key) {
      return record.subarray(keyEnd + 2, keyEnd + 2 + valueLen);
    }
    at = keyEnd + 2 + valueLen;
  }
  return null;
}

/** The settings record the runtime holds (SETTINGS, read in pieces of 768 bytes). */
export async function readSettingsRecord(dk: DkSession): Promise<Uint8Array> {
  const out: number[] = [];
  let total: number | null = null;
  while (total === null || out.length < total) {
    const body = new Uint8Array(6);
    const view = new DataView(body.buffer);
    view.setUint32(0, out.length);
    view.setUint16(4, 768);
    const reply = await dk.request(CMD.SETTINGS, body);
    if (reply.status !== 0 || reply.body.length <= 10) throw new Error(`SETTINGS: status ${reply.status}`);
    const head = new DataView(reply.body.buffer, reply.body.byteOffset, reply.body.byteLength);
    total ??= head.getUint32(6);
    out.push(...reply.body.subarray(10));
  }
  return Uint8Array.from(out);
}

export interface Fault {
  kind: 'cpu' | 'dsp' | 'panic';
  site: number;
  stage: number;
  id: number;
  step: number;
  at: number;
  runtime: number;
  module: string;
}

/** The 40 bytes of `dk/fault` (core/runtime sdk `fault.rs`): `FLT1`, kind | site << 8 | stage << 16, id, module step, PC or last word, runtime, name. */
export function decodeFault(value: Uint8Array): Fault | null {
  if (value.length < 40) return null;
  const view = new DataView(value.buffer, value.byteOffset, value.byteLength);
  if (view.getUint32(0) !== 0x464c5431) return null;
  const k = view.getUint32(4);
  const kind = k & 0xff;
  if (kind !== 1 && kind !== 2 && kind !== 3) return null;
  const name = String.fromCharCode(...value.subarray(24, 40)).split('\0')[0]!;
  return {
    kind: kind === 1 ? 'cpu' : kind === 2 ? 'dsp' : 'panic',
    site: (k >> 8) & 0xff,
    stage: (k >> 16) & 0xff,
    id: view.getUint32(8),
    step: view.getUint32(12),
    at: view.getUint32(16),
    runtime: view.getUint32(20),
    module: name,
  };
}
