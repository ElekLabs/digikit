import { pollUntil } from '@digi/test-support';
import { expect, test } from 'vitest';
import { cardRig, RT } from '@digi/regression/runtime/card-rig.ts';
import { FIRMWARE, describeOn } from '@digi/regression/runtime/devices.ts';
import { pickSine, word } from '@digi/regression/runtime/fx-live.ts';
import { dynType } from '@digi/regression/runtime/project-kit.ts';
import { DN2_TX_TYPE_TRACK1_112, expectLoaded, tableSymbols } from './machines-kit.ts';

/**
 * The frame type hook of the Digitone II (`cf.machine.hook.frametype`, an interrupt site of the frame builder; the routine `mf_frametype` of `machines.dks`): the firmware
 * writes the machine type of track 1 into the halfword at TX frame word 0x94 of the DSP frame. Without the service the stock code runs and the frame holds FM TONE's type 0;
 * with `machines.dks` and a machine with its own DSP part (`test/dyn-dn2`) the frame holds the machine's own type, not its clone_of. Only the TX frame shows this.
 * Two boots: the plain runtime, then the card with the machine. The rest of the machine (rows, pages, knobs) is `machine-rows-dn2`.
 */
const ID = 'test/dyn-dn2';

describeOn('dn2', `DN2 ${FIRMWARE.dn2} frame type hook`, () => {
  const run = cardRig('dn2', 'frametype-slot', {
    dsp: 'sharc',
    roots: RT,
    symbols: tableSymbols('dn2'),
    initial: () => ({}),
  });

  test('the TX frame holds type 0 on a plain boot and the own type of a machine with its own DSP part when machines.dks is loaded', async () => {
    const plain = await run.first();
    expect((await word(plain.session, DN2_TX_TYPE_TRACK1_112)) >>> 16, 'no service: the stock code sends FM TONE type').toBe(0);

    const rt = await run.load('with-machines', { modules: [ID], selection: [run.mod(ID).name] });
    const { session } = rt;
    await expectLoaded(rt, run);
    const type = await dynType(session, run.plan, ID, 'dyn_pluck');
    expect(type, 'a run-time type (the range of core/rows)').toBeGreaterThanOrEqual(16);
    // FUN+SRC, five DOWN, YES, YES as in machine-rows-dn2; the TRIG sends the type to the audio side
    await pickSine(rt, 5);
    await rt.ui.tap('TRIG 1', { until: { drained: true } });
    const tx = await pollUntil(rt.clock, () => word(session, DN2_TX_TYPE_TRACK1_112), (w) => w >>> 16 === type, {
      withinMs: 3000,
      message: 'the TX frame carries the machine type of track 1',
      intervalMs: 50,
    });
    expect(tx >>> 16, 'the hook leaves the own type in the frame').toBe(type);
  });
});
