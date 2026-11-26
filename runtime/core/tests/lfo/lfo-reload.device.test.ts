import { L_E_BOOT_ONLY } from '@digi/dk-session/gen_bulk';
import { expect, test } from 'vitest';
import { CARD_SYMBOLS, cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { bootWith, shapeWords, SHAPE_A } from './lfo-cards.ts';
import { describeSamples, evalTable, fitShape, FIRST_SHAPE, KNOB_WORD, LfoRig, OK, RAMP_SAMPLES } from './lfo-rig.ts';

/**
 * Hot reload of the lfo service is refused (hot reload slice 1, guard): the tick of the service is entered from the block interrupt (vector 191), which no task can be parked out of,
 * so `RELOAD services/lfo.dks` answers `E_BOOT_ONLY` (the menu shows RESTART TO LOAD) before anything is taken down. With the RAMP shape loaded and WAVE on its number the LFO output follows
 * the shape's table before and after the refused job (`fitShape`: free rate and start phase, so the proof is the fit error), the wave numbers are the same and nothing faults. The check
 * itself is a host test (`provider_reload.rs`); this one needs the firmware to show that nothing was taken down. The hang of an accepted lfo reload (the UNLOAD step never returns on DN2)
 * is an open item (ports/fw/README.md).
 */
const READS = 100;
const GAP_MS = 12;

cardRigs('lfo reload', { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true }, (h) => {
  test('RELOAD services/lfo.dks is refused with E_BOOT_ONLY and the lfo keeps running', async () => {
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
    expect(end, 'the reload of the lfo service is refused before anything is taken down').toEqual({ result: L_E_BOOT_ONLY, unloaded: false });

    expect(await rig.word(KNOB_WORD.WAVE), 'the WAVE knob word is unchanged').toBe(waveBefore);
    const second = await rig.sample(dest, READS, GAP_MS);
    const fitAfter = fitShape(second, (p) => evalTable(RAMP_SAMPLES, p));
    expect(fitAfter.err, `after: the output still follows the RAMP table (samples ${describeSamples(second)})`).toBeLessThan(OK);
    expect(await shapeWords(h, rt), 'the sticky map holds the same wave numbers').toEqual(words);
    console.info(`lfo reload guard ${h.device}: result ${end.result}; fit error ${fitBefore.err.toFixed(4)} -> ${fitAfter.err.toFixed(4)}`);
    await rt.session.expectNoFault('lfo reload refused');
  }, 900_000);
});
