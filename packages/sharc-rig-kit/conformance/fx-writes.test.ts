import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { FxRig, ROOT } from '../src/index.ts';

describe('the write-set check', () => {
  // tests/support/fx_effects.s: `_te_stray` stores into the master block, which is read only for an effect; `_te_half` stores only its output
  const sources = [path.join(ROOT, 'tests/support/fx_effects.s')];
  const found = async (pc: string) => {
    const rig = await FxRig.open([{ pc, state: '_te_state_a' }], {
      sources,
      recordStores: true,
    });
    try {
      return rig.foreignStores(16, (await rig.block(Array(64).fill(0.25))).stores);
    } finally {
      await rig.close();
    }
  };

  it('finds a store into the input block', async () => {
    const stray = await found('_te_stray');
    expect(stray.length).toBeGreaterThan(0);
    expect(stray[0]).toContain('master block');
  });

  it('finds nothing in a well-behaved effect', async () => {
    expect(await found('_te_half')).toEqual([]);
  });
});
