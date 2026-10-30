// Types of fakeflash.mjs, for its typed user apps/desktop/ui/preview/firmware-mock.ts.
export const RPC_START: number;
export const RPC_WRITE: number;
export const RPC_END: number;
export function crc32(data: Iterable<number>): number;

export interface FakeUpgradeOptions {
  writeMs?: number;
  checkMs?: number;
  endMs?: number;
  errorAt?: number | null;
}

export interface FakeUpgradeAnswer {
  message: Uint8Array;
  delayMs: number;
  /** 1 or 0 for End. */
  commit?: number;
}

export class FakeUpgrade {
  constructor(options?: FakeUpgradeOptions);
  endMs: number;
  writeMs: number;
  checkMs: number;
  errorAt: number | null;
  counter: number;
  total: number;
  taken: number;
  ends: number[];
  static is(msg: Uint8Array): boolean;
  reply(seq: number, ty: number, payload: ArrayLike<number>): Uint8Array;
  handle(msg: Uint8Array): FakeUpgradeAnswer | null;
}
