import { expect, test } from 'vitest';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { bootWith, shapeWords, SHAPE_A } from './lfo-cards.ts';
import { describeSamples, evalTable, fitShape, FIRST_SHAPE, KNOB_WORD, LfoRig, OK, RAMP_SAMPLES } from './lfo-rig.ts';

/**
 * Hot reload of the lfo service (hot reload, shape table): `RELOAD services/lfo.dks` answers OK. The job takes the shape module down and the service with it, then brings the service up
 * from the file and the shape module back. With the RAMP shape loaded and WAVE on its number the LFO output follows the shape's table before and after the job (`fitShape`: free rate and
 * start phase, so the proof is the fit error), the wave number is the same and nothing faults. The order of the steps and the refusals are host tests (`dkrt-lifecycle`, `provider_reload.rs`);
 * this one shows what only the firmware shows: the shape table is republished under the interrupt mask and the vector 191 tick plays it again. The module log is not listened to here.
 */
const READS = 100;
const GAP_MS = 12;

cardRigs('lfo reload', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('RELOAD services/lfo.dks answers OK and the lfo plays the shape after it', async () => {
    const rt = await bootWith(h, 'lfo-reload', [SHAPE_A]);
    const words = await shapeWords(h, rt);
    expect(words[0]! & 0xff, 'the shape has its number').toBe(FIRST_SHAPE);
    const rig = new LfoRig(rt);
    const dest = await rig.run();
    await rig.setWave(FIRST_SHAPE);
    const first = await rig.sample(dest, READS, GAP_MS);
    const fitBefore = fitShape(first, (p) => evalTable(RAMP_SAMPLES, p));
    expect(fitBefore.err, `before: the output follows the RAMP table (samples ${describeSamples(first)})`).toBeLessThan(OK);
    const waveBefore = await rig.word(KNOB_WORD.WAVE);

    const end = await h.usb(rt, (dk) => dk.reloadAndWait('services/lfo.dks'));
    expect(end.result, 'the reload of the lfo service answers OK').toBe(0);
    expect(await rig.word(KNOB_WORD.WAVE), 'the WAVE knob word is unchanged').toBe(waveBefore);
    const second = await rig.sample(dest, READS, GAP_MS);
    const fitAfter = fitShape(second, (p) => evalTable(RAMP_SAMPLES, p));
    expect(fitAfter.err, `after: the output follows the RAMP table (samples ${describeSamples(second)})`).toBeLessThan(OK);
    expect(await shapeWords(h, rt), 'the sticky map holds the same wave numbers').toEqual(words);
    console.info(`lfo reload ${h.device}: result ${end.result}; fit error ${fitBefore.err.toFixed(4)} -> ${fitAfter.err.toFixed(4)}`);
    await rt.session.expectNoFault('lfo reload');
  }, 900_000);
});
