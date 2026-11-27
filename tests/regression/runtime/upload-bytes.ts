import { DkError, fileHashHex } from '@digi/dk-session';
import { LOAD_STATUS, STATUS } from '../../../packages/dk-session/src/gen_proto.ts';

// ---- the upload protocol ------------------------------------------------------------------------------------------------------------

/**
 * The status a protocol call ends with, as a number (0 for success). A DkSession call throws a DkError with the status name; the number comes
 * from the generated tables. Other errors are rethrown.
 */
export async function statusOf(op: () => Promise<unknown>): Promise<number> {
  try {
    await op();
    return 0;
  } catch (e) {
    const row =
      e instanceof DkError && e.kind === 'device'
        ? [...STATUS, ...LOAD_STATUS].find(([, name]) => name === e.status)
        : undefined;
    if (!row) throw e;
    return row[0];
  }
}

/** The store's hash slot of `b` (hex): what LIST, VERIFY and the card's table say of it. */
export const sha = (b: Uint8Array): string => fileHashHex(b);

/** Deterministic bytes with every value (the 8-in-7 packing needs the high bits): mulberry32 of `seed`. */
export function blob(n: number, seed: number): Buffer {
  const out = Buffer.alloc(n);
  let a = seed >>> 0;
  for (let i = 0; i < n; i++) {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), a | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    out[i] = (t ^ (t >>> 14)) & 0xff;
  }
  return out;
}
