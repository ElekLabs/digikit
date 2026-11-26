import { expect, test } from 'vitest';
import { CARD_SYMBOLS, cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { bootWith, shapeWords, SHAPE_A } from './lfo-cards.ts';
import { describeSamples, evalTable, fitShape, FIRST_SHAPE, KNOB_WORD, LfoRig, OK, RAMP_SAMPLES } from './lfo-rig.ts';

/**
 * Hot reload of the lfo service (hot reload slice 1, DF3): with the RAMP shape loaded and WAVE on its number, `RELOAD services/lfo.dks` takes the shape module and the service down and
 * brings them back from the file on the card. The service's keys, wave numbers and the LFO output must come back as they were: the sticky map `lfo.shapes` is the same, the WAVE knob
 * word is the same, and the samples read after the reload fit the shape's table as the samples before did (`fitShape`: the fit has free rate and start phase, so the proof is the
 * fit error, not a sample-exact match; the numbers are printed). No sound is pinned. The job's steps are host tests; this one needs the firmware's LFO tick and its stored function
 * pointers (the rows the service wrote are re-pointed at the new copy).
 */
const READS = 100;
const GAP_MS = 12;

cardRigs('lfo reload', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('after RELOAD services/lfo.dks the shape keeps its number and the output follows its table', async () => {
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
    expect(end, 'the reload of the lfo service ran to its end').toEqual({ result: 0, unloaded: true });

    // the rig is not set up again: the destination, speed and wave are the firmware's own state, which the reload must leave alone
    const waveAfter = await rig.word(KNOB_WORD.WAVE);
    expect(waveAfter, 'the WAVE knob word is unchanged').toBe(waveBefore);
    const second = await rig.sample(dest, READS, GAP_MS);
    const fitAfter = fitShape(second, (p) => evalTable(RAMP_SAMPLES, p));
    expect(fitAfter.err, `after: the output follows the RAMP table (samples ${describeSamples(second)})`).toBeLessThan(OK);
    expect(await shapeWords(h, rt), 'the sticky map holds the same wave numbers').toEqual(words);
    const span = (s: { v: number }[]) => Math.max(...s.map((x) => x.v)) - Math.min(...s.map((x) => x.v));
    console.info(
      `DF3 ${h.device}: wave word ${waveBefore.toString(16)} -> ${waveAfter.toString(16)}; fit error ${fitBefore.err.toFixed(4)} -> ${fitAfter.err.toFixed(4)}; swing ${span(first)} -> ${span(second)}; shapes ${words.map((w) => w.toString(16)).join(' ')}`,
    );
    await rt.session.expectNoFault('lfo reload');
  }, 900_000);
});
