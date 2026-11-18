import { expect, test } from 'vitest';
import { golden } from '@digi/test-support';
import { CARD_SYMBOLS, cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { stableFrameHash } from '@digi/regression/runtime/fx-live.ts';
import { captureRemoteOled } from '@digi/regression/runtime/remote.ts';
import { bootWith, GOLDENS, SHAPE_A, SHAPE_B } from './lfo-cards.ts';
import { FIRST_SHAPE, LfoRig, WAVE } from './lfo-rig.ts';

/**
 * E6 on the device, the panel (docs/plans/extension-model.md E6, tests list): with two shapes loaded (RAMP is wave 7, STEP is 8) the WAVE encoder of the LFO page goes past
 * RAND to the shape numbers and stops at the highest loaded number (the `t1_range` hook raises the maximum); the name and the graph of the MOD page show the shape.
 * The Sound word says where the encoder stopped (firmware state). The picture is the one thing only the screen shows: each page is captured (the PNG paths are printed) and its
 * frame hash is compared with `goldens.json`, which holds the hashes after the pictures were looked at (`UPDATE_GOLDENS=1` writes them; the first run of a device needs it once).
 * The pages must also differ from each other, which needs no golden: RAND (name RND), RAMP and STEP differ in name and graph. No SHARC.
 */
cardRigs('lfo shapes: panel', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('WAVE goes past RAND to the shape numbers, stops at the highest, and the page shows name and graph', async () => {
    const rt = await bootWith(h, 'panel', [SHAPE_A, SHAPE_B]);
    const device = rt.session.options.device;
    const rig = new LfoRig(rt);
    await rig.open();
    const waveIndex = await rig.locate('WAVE');
    const page = async (name: string) => {
      const hash = await stableFrameHash(rt);
      console.info(`${device} ${name} screen: ${await captureRemoteOled(rt.session, `lfo-${name}`)}`);
      return hash;
    };

    await rig.setWave(WAVE.RAND);
    const rand = await page('rand');
    // one detent past RAND reaches the first shape: the stock maximum would stop at RAND
    await rig.setWave(FIRST_SHAPE);
    const ramp = await page('ramp');
    await rig.setWave(FIRST_SHAPE + 1);
    const step = await page('step');
    expect(new Set([rand, ramp, step]).size, 'RND, RAMP and STEP show three different pages (name and graph)').toBe(3);

    // the highest loaded number is the end of the range: three more detents change nothing
    await rig.turn('WAVE', 1, 3);
    expect(await rig.word(waveIndex), 'WAVE stops at the highest loaded number').toBe((FIRST_SHAPE + 1) << 8);
    await expect(rig.turnTo('WAVE', (FIRST_SHAPE + 2) << 8)).rejects.toThrow(/stops at/);
    expect(await page('step-top'), 'the page is the same at the end of the range').toBe(step);

    golden(GOLDENS, `${device}.wave7-RAMP`, ramp);
    golden(GOLDENS, `${device}.wave8-STEP`, step);
    await rt.session.expectNoFault('LFO panel with shapes');
  }, 900_000);
});
