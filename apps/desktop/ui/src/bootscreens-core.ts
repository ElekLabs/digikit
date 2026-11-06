// Header preflight only. The pack tool and device own full DKBS record/CRC validation.
import type { DkSession } from '@digi/dk-session';

export const ACTIVE_BOOTSCREEN = 'bootscreens/active.dkbs';
export interface Bootscreen {
  name: string;
  bytes: Uint8Array;
  records: number;
  ticks: number;
}

export function preflightBootscreen(bytes: Uint8Array): { records: number; ticks: number } {
  if (bytes.length < 16 || bytes.length > 65536) throw new Error('Choose a DKBS file of 16–65536 bytes.');
  const h = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (h.getUint32(0) !== 0x444b4253 || h.getUint16(4) !== 1) throw new Error('Choose a DKBS version 1 file.');
  const records = h.getUint16(6);
  const ticks = h.getUint32(8);
  if (!records || !ticks || ticks > 300 || records > ticks || bytes.length < 16 + records * 8)
    throw new Error('The DKBS header has invalid record or tick bounds.');
  return { records, ticks };
}

export async function readBootscreen(file: Pick<File, 'name' | 'size' | 'arrayBuffer'>): Promise<Bootscreen> {
  if (file.size < 16 || file.size > 65536) throw new Error('Choose a DKBS file of 16–65536 bytes.');
  const bytes = new Uint8Array(await file.arrayBuffer());
  return { name: file.name, bytes, ...preflightBootscreen(bytes) };
}

export async function uploadBootscreen(
  session: Pick<DkSession, 'put' | 'verifyFile'>,
  bytes: Uint8Array,
  progress: (sent: number, total: number) => void,
  verifying: () => void,
): Promise<void> {
  preflightBootscreen(bytes);
  await session.put(ACTIVE_BOOTSCREEN, bytes, progress);
  verifying();
  if (!(await session.verifyFile(ACTIVE_BOOTSCREEN, bytes)))
    throw new Error('Verification failed: the bootscreen on the card differs.');
}

export async function removeBootscreen(session: Pick<DkSession, 'delete'>): Promise<void> {
  await session.delete(ACTIVE_BOOTSCREEN);
}
