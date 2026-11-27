/**
 * The render stub of core/dsp-voice on DN2 (dsp-voice-dn2.s) with the dispatch code of the machines service (dispatch-dn2.s) on the SHARC-only rig.
 * The stub reads the slot `voice_render` of core/dsp-link: zero takes the stock helper and keeps the heartbeat, a set slot dispatches.
 */
import { describe, expect, it } from 'vitest';
import { MachineRig, STOCK_BP, TABLE, machineSpecs, nw, w2f } from '@digi/sharc-rig-kit';

const spec = machineSpecs().find((s) => s.device === 'dn2' && s.noteOn !== null)!;
const nan = (x: number): boolean => (x >>> 0) === 0xffffffff || Number.isNaN(x);

describe('core/dsp-voice render stub on DN2', () => {
  it('a zero slot takes the stock helper, writes nothing and keeps the heartbeat', async () => {
    const rig = await MachineRig.open(spec);
    try {
      rig.setSlot(0, 0);
      const diag = rig.built.data._dv_diag!;
      rig.setParam(spec.params[0]!.id, spec.params[0]!.default);
      rig.trigger();
      const a = await rig.block('friendly', [[diag, 1]]);
      rig.trigger();
      const b = await rig.block('friendly', [[diag, 1]]);
      for (const out of [a, b]) {
        expect(out.returned, 'the call ends at the stock helper').toBe(true);
        expect(out.finalPc).toBe(STOCK_BP);
        expect(out.samples.every(nan), 'no dispatch: the output block is untouched').toBe(true);
        expect(out.contextBad).toEqual([]);
      }
      expect((b.extra[0]![0]! - a.extra[0]![0]!) >>> 0, 'voice 0 counts the block').toBe(1);
      expect(a.instructions, 'the stub on voice 0: heartbeat, context words and the slot test (50 measured)').toBeLessThan(56);
    } finally {
      await rig.close();
    }
  });

  it('a set slot dispatches and the entry restores M13 and I12', async () => {
    const rig = await MachineRig.open(spec);
    try {
      rig.trigger();
      await rig.block('friendly');
      rig.trigger();
      const out = await rig.block('friendly');
      expect(out.returned).toBe(true);
      expect(out.contextBad).toEqual([]);
      expect(out.samples.some((x) => !nan(x)), 'the machine rendered').toBe(true);
    } finally {
      await rig.close();
    }
  });

  it('a row whose disabled word names its type renders silence, as the MISSING placeholder of the machines service does', async () => {
    const rig = await MachineRig.open(spec);
    try {
      // the placeholder row: type and pc as published, word 10 pre-set to the type
      rig.poke(nw(TABLE) + 10, spec.machineType);
      rig.trigger();
      await rig.block('friendly');
      rig.trigger();
      const out = await rig.block('friendly');
      expect(out.returned).toBe(true);
      expect(out.contextBad).toEqual([]);
      expect(out.samples.every((x) => !nan(x) && w2f(x) === 0), 'a disabled type is a silent block').toBe(true);
    } finally {
      await rig.close();
    }
  });
});
