import { describe, expect, test } from 'vitest';
import { HOST_COUNT, HOST_DIR_MAGIC, HOST_DIR_READY, HOST_ID } from '@digi/dk-session';
import type { Session } from '@digi/test-support';
import { packageBlock, providerBlocks } from '../runtime/memaudit-kit.ts';
import type { RuntimePlan } from '../runtime/runtime-plan.ts';

const DIR_AT = 0x4000_0400;

/** A session whose memory is the host directory the core leaves: the magic, the count, the ready word and the entries. */
function sessionWith(ready: boolean) {
  const words = new Map<number, number>();
  const put = (at: number, value: number) => words.set(at, value);
  put(DIR_AT, HOST_DIR_MAGIC);
  put(DIR_AT + 4, HOST_COUNT);
  put(DIR_AT + 8, ready ? HOST_DIR_READY : 0);
  for (const [name, value] of Object.entries({
    PACKAGE_LO: 0x4010_0000,
    PACKAGE_HI: 0x4011_0000,
    PROVIDERS: 0x4012_0000,
  }))
    put(DIR_AT + 12 + 4 * HOST_ID[name as keyof typeof HOST_ID], value);
  const client = { memPeek: async (at: number) => words.get(at) ?? 0 };
  const plan = { symbols: { 'core/runtime@0.1.0:DKRT_HOST_DIR': DIR_AT } } as unknown as RuntimePlan;
  return { session: { client } as unknown as Session, plan };
}

describe('memaudit and the host directory', () => {
  test('the package block is the span the core published once the directory is ready', async () => {
    const { session, plan } = sessionWith(true);
    expect(await packageBlock(session, plan)).toEqual({
      name: 'system package',
      cf_base: 0x4010_0000,
      cf_size: 0x1_0000,
    });
  });

  test('a directory that is not ready is no package: no block, no providers, whatever its entries hold', async () => {
    const { session, plan } = sessionWith(false);
    await expect(packageBlock(session, plan)).rejects.toThrow(/no package loaded/);
    await expect(providerBlocks(session, plan)).rejects.toThrow(/no package loaded/);
  });
});
