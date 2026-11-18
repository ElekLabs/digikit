import { describe, expect, test } from 'vitest';
import { CAP } from '../src/gen_proto.ts';
import type { DkInfoFull } from '../src/dkupload-core.ts';
import { openSamples, skipVerifyAfterBulk } from '../src/samples.ts';

const info = (caps: number | null) => ({ caps }) as unknown as DkInfoFull;
const withReadback = info(1 << CAP.READBACK);
const without = info(1 << CAP.RENAME);

const store = (bulk: boolean) =>
  openSamples('digikit', { session: {} as never, bulk: bulk ? ({} as never) : undefined });
const stock = (bulk: boolean) =>
  openSamples('stock-rpc', { session: {} as never, hashOf: () => 0, bulk: bulk ? ({} as never) : undefined });

describe('skipVerifyAfterBulk', () => {
  test('the store skips the SysEx verify after a bulk END only with READBACK', () => {
    expect(skipVerifyAfterBulk('auto', store(true), withReadback)).toBe(true);
    expect(skipVerifyAfterBulk('auto', store(true), without)).toBe(false);
    expect(skipVerifyAfterBulk('auto', store(true), info(null))).toBe(false);
    expect(skipVerifyAfterBulk('auto', store(true), null)).toBe(false);
  });
  test('always keeps the verify, never skips it, a non-bulk put keeps it', () => {
    expect(skipVerifyAfterBulk('always', store(true), withReadback)).toBe(false);
    expect(skipVerifyAfterBulk('never', store(false), without)).toBe(true);
    expect(skipVerifyAfterBulk('auto', store(false), withReadback)).toBe(false);
  });
  test('the +Drive backend skips on bulk without asking INFO and keeps the verify on another link', () => {
    expect(skipVerifyAfterBulk('auto', stock(true), null)).toBe(true);
    expect(skipVerifyAfterBulk('auto', stock(false), null)).toBe(false);
    expect(skipVerifyAfterBulk('always', stock(true), null)).toBe(false);
  });
});
