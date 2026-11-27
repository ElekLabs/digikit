import { expect, test } from 'vitest';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { REDRAW, stableFrameHash } from '@digi/regression/runtime/panel-waits.ts';
import { captureRemoteOled } from '@digi/regression/runtime/remote.ts';
import { bootWith, pinFrame, SHAPE_A, SHAPE_B } from './lfo-cards.ts';
import { FIRST_SHAPE, KNOB_WORD, LfoRig, WAVE } from './lfo-rig.ts';

/**
 * E6 on the device, the panel (docs/plans/extension-model.md E6, tests list): with two shapes loaded (RAMP is wave 7, STEP is 8) the WAVE encoder of the LFO page goes past
 * RAND to the shape numbers and stops at the highest loaded number (the `t1_range` hook raises the maximum); the name and the graph of the MOD page show the shape.
 * The Sound word says where the encoder stopped (firmware state). The picture is the one thing only the screen shows: each page is captured (the PNG paths are printed) and its
 * frame hash is compared with `goldens.json`, which holds the hashes after the pictures were looked at (`UPDATE_GOLDENS=1` writes them; the first run of a device needs it once).
 * Every frame is taken with the LFO stopped. The pages must also differ from each other, which needs no golden: RAND (name RND), RAMP and STEP differ in name and graph. No SHARC.
 */
cardRigs('lfo shapes: panel', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('WAVE goes past RAND to the shape numbers, stops at the highest, and the page shows name and graph', async () => {
    const rt = await bootWith(h, 'panel', [SHAPE_A, SHAPE_B]);
    const device = rt.session.options.device;
    const rig = new LfoRig(rt);
    // run() starts an output with MIDI CC; freeze() stops the LFO, whose graph marker would change every frame hash
    const dest = await rig.run();
    await rig.freeze(dest);
    await rig.open();
    const shots: Record<string, string> = {};
    const page = async (name: string) => {
      await rt.ui.wait({ ms: 300, why: REDRAW });
      const hash = await stableFrameHash(rt);
      shots[name] = await captureRemoteOled(rt.session, `lfo-${name}`);
      console.info(`${device} ${name} screen: ${shots[name]}`);
      return hash;
    };

    // A CC does not redraw the WAVE label (the name shows after a page redraw), so every capture follows a knob turn
    // a CC only sets the start value; the last change before each capture is a knob turn
    await rig.setWave(FIRST_SHAPE);
    await rig.turnWaveTo(WAVE.RAND);
    const rand = await page('rand');
    // the encoder goes past RAND: the stock maximum would stop at RAND
    await rig.turnWaveTo(FIRST_SHAPE);
    const ramp = await page('ramp');
    await rig.turnWaveTo(FIRST_SHAPE + 1);
    const step = await page('step');
    expect(new Set([rand, ramp, step]).size, 'RND, RAMP and STEP show three different pages (name and graph)').toBe(3);

    // the highest loaded number is the end of the range: more detents change nothing
    await rig.turnWave(8, 4, 30);
    expect(await rig.word(KNOB_WORD.WAVE), 'WAVE stops at the highest loaded number').toBe((FIRST_SHAPE + 1) << 8);
    await rig.turnWaveTo(FIRST_SHAPE + 1);
    expect(await page('step-top'), 'the page is the same at the end of the range').toBe(step);

    for (const [key, hash, name] of [
      ['ramp', ramp, 'wave7-RAMP'],
      ['step', step, 'wave8-STEP'],
    ] as const) {
      const png = shots[key];
      if (png === undefined) throw new Error(`no capture path for the ${key} page`);
      pinFrame(`${device}.${name}`, hash, png);
    }
    await rt.session.expectNoFault('LFO panel with shapes');
  }, 900_000);
});
