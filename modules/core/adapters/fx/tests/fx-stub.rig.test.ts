/**
 * The hook stubs of core/fx (fx.s: the master hook, fxsend.s: the send hook) with the dispatch code of the fx service
 * (core/services/fx/dks/dsp/fx-dt2.s) on the SHARC-only rig. A zero slot takes the stock path with every register as found and the
 * master untouched; a filled slot runs the chain.
 */
import { describe, expect, it } from 'vitest';
import { FX_ENTRY_SLOT, FX_SEND_SLOT } from '@digi/sharc-rig-kit';
import { RAMP, SIGNALS, clean, load, same, sendRow, withRig } from './fx-send-model.ts';

const ADD1 = { pc: '_te_add1', state: '_te_state_a' };

describe('core/fx hook stubs', () => {
  it(
    'a zero fx_entry slot leaves the master untouched and returns every register',
    withRig([ADD1], async (rig) => {
      rig.setHookSlot(FX_ENTRY_SLOT, 0);
      const b = await rig.block(RAMP, 'friendly');
      clean(b);
      same(b.master, RAMP);
      expect(b.instructions, 'the stub: R0 saved, the slot read, R0 back, the tail (6 measured)').toBeLessThanOrEqual(6);
    }),
  );

  it(
    'a set fx_entry slot runs the chain',
    withRig([ADD1], async (rig) => {
      await rig.block(RAMP);
      const b = await rig.block(RAMP);
      clean(b);
      expect(b.master).not.toEqual(RAMP);
    }),
  );

  it(
    'a zero fx_send slot leaves the master and the return alone',
    withRig([sendRow()], async (rig) => {
      load(rig);
      rig.setHookSlot(FX_SEND_SLOT, 0);
      const b = await rig.sendBlock(RAMP, 'friendly');
      clean(b);
      same(b.master, RAMP);
      same(b.ret, Array<number>(64).fill(0));
      expect(b.instructions, 'the send stub with nothing installed (6 measured)').toBeLessThanOrEqual(6);
      expect(SIGNALS.length).toBe(16);
    }),
  );

  it(
    'a set fx_send slot adds the send row to the return',
    withRig([sendRow()], async (rig) => {
      load(rig);
      await rig.sendBlock();
      const b = await rig.sendBlock(RAMP, 'friendly');
      clean(b);
      expect(b.ret.some((x) => x !== 0)).toBe(true);
    }),
  );
});
