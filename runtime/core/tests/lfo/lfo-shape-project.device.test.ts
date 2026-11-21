import { expect, test } from 'vitest';
import { CARD_SYMBOLS, cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { REDRAW, stableFrameHash } from '@digi/regression/runtime/fx-live.ts';
import { loadPanelProject, savePanelProject } from '@digi/regression/runtime/project-kit.ts';
import { captureRemoteOled } from '@digi/regression/runtime/remote.ts';
import { servicePath } from '@digi/regression/runtime/service-files.ts';
import { bootWith, pinFrame, shapeWords, SHAPE_A, SHAPE_B, storePath } from './lfo-cards.ts';
import { BAD, evalSine, evalSqr, evalTable, evalTri, fitShape, FIRST_SHAPE, KNOB_WORD, LfoRig, OK, RAMP_SAMPLES, WAVE } from './lfo-rig.ts';

/**
 * E6 on the device: a project that holds a shape number, loaded where the shape or the service is missing (docs/plans/extension-model.md E6, item 4 of "What" and the tests
 * list). Rule 7: the extended wave value is stored in the sound; every stock reader clamps it, so the sound loads, shows ERR and plays as RAND, with no fault, and the
 * value survives save and load. With the service loaded and the shape's module absent, the tick plays TRI and the page shows "----".
 * The project is made on the panel (the same save and load as the project round trip), with the LFO running at a speed the fit can follow, so the fit reads the
 * loaded project's own LFO. Safe mode loads no module and no service: it is the same code path as the case "no lfo.dks" and is not run separately (safe-mode.device.test.ts
 * proves that a boot in safe mode loads nothing). No SHARC (the zero-DSP session paces the vector 191 tick). The LFO output is read from the live array (`LIVE` in lfo-rig.ts).
 * Every frame hash is taken with the LFO stopped (`freeze`): the graph of the page draws a marker that follows the LFO phase.
 */
const READS = 100;
const GAP_MS = 12;

cardRigs(
  'lfo shapes: projects without the shape or the service',
  { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), project: true, uploads: true, relaxFs: true },
  (h) => {
    /** The frame of the MOD page of LFO 1 with the wave of the project loaded. */
    const modPage = async (rig: LfoRig, dest: number, label: string) => {
      await rig.open();
      await rig.freeze(dest);
      const hash = await stableFrameHash(rig.rt);
      console.info(`MOD page screen (${label}): ${await captureRemoteOled(rig.rt.session, `lfo-mod-${label}`)}`);
      return hash;
    };

    test('no lfo.dks: a project with a shape number loads without a fault, plays as RAND, shows ERR and keeps the value through a save and a load', async () => {
      const rt = await bootWith(h, 'project', [SHAPE_A]);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      await rig.setWave(FIRST_SHAPE);
      await savePanelProject(rt);
      rig.closed();

      // the same card without the module and without the service file
      await h.usb(rt, async (dk) => {
        await dk.select([]);
        await dk.delete(servicePath('lfo'));
      });
      const rt2 = await h.restart(rt);
      expect(await h.usb(rt2, (dk) => dk.modules()), 'no module is loaded').toEqual([]);
      await loadPanelProject(rt2);
      const rig2 = new LfoRig(rt2);
      // the loader writes the destination through its remap table: the array index after the load is read back, not assumed
      const dest2 = await rig2.destination();
      console.info(`destination before the save ${dest}, after the load ${dest2}`);
      expect(await rig2.word(KNOB_WORD.WAVE), 'the loader keeps the wave number: nothing clamps it').toBe(FIRST_SHAPE << 8);
      const samples = await rig2.sample(dest2, READS, GAP_MS);
      const note = `TRI ${fitShape(samples, evalTri).err.toFixed(3)}, SQR ${fitShape(samples, evalSqr).err.toFixed(3)}, SINE ${fitShape(samples, evalSine).err.toFixed(3)}`;
      for (const [name, shape] of [
        ['TRI', evalTri],
        ['SQR', evalSqr],
        ['the RAMP table', (p: number) => evalTable(RAMP_SAMPLES, p)],
      ] as const)
        expect(fitShape(samples, shape).err, `the stock tick plays RAND, not ${name} (${note})`).toBeGreaterThan(BAD);
      // the LFO is not stopped: the destination moves
      expect(new Set(samples.map((s) => s.v)).size, 'RAND moves the destination').toBeGreaterThan(1);
      const err = await modPage(rig2, dest2, 'err-before');
      console.info(`ERR page screen: ${await captureRemoteOled(rt2.session, 'lfo-err-page')}`);

      // the value survives a save and a load of the project
      await savePanelProject(rt2);
      rig2.closed();
      await loadPanelProject(rt2);
      rig2.closed();
      expect(await rig2.word(KNOB_WORD.WAVE), 'the number survives save and load').toBe(FIRST_SHAPE << 8);
      expect(await modPage(rig2, dest2, 'err-after'), 'the page is the same after the load').toBe(err);
      await rt2.session.expectNoFault('project with a shape number, no lfo.dks');
    }, 900_000);

    test('lfo.dks loaded, the shape missing: the tick plays TRI and the name shows ----', async () => {
      const rt = await bootWith(h, 'two', [SHAPE_A, SHAPE_B]);
      const numbers = (await shapeWords(h, rt)).map((w) => w & 0xff);
      expect(numbers.slice(0, 2), 'RAMP gets 7 and STEP gets 8').toEqual([FIRST_SHAPE, FIRST_SHAPE + 1]);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      await rig.setWave(FIRST_SHAPE);
      await savePanelProject(rt);
      rig.closed();

      // RAMP is gone, STEP stays: the service runs (STEP needs it) and number 7 is a gap below the highest number 8
      await h.usb(rt, (dk) => dk.select([storePath(h.device, SHAPE_B)]));
      const rt2 = await h.restart(rt);
      expect((await h.usb(rt2, (dk) => dk.modules())).map((m) => m.name), 'only STEP is loaded').toEqual([SHAPE_B]);
      expect((await shapeWords(h, rt2)).map((w) => w & 0xff), 'STEP keeps number 8').toContain(FIRST_SHAPE + 1);
      await loadPanelProject(rt2);
      const rig2 = new LfoRig(rt2);
      // the loader writes the destination through its remap table: the array index after the load is read back, not assumed
      const dest2 = await rig2.destination();
      console.info(`destination before the save ${dest}, after the load ${dest2}`);
      expect(await rig2.word(KNOB_WORD.WAVE), 'the project holds wave 7').toBe(FIRST_SHAPE << 8);
      const samples = await rig2.sample(dest2, READS, GAP_MS);
      const tri = fitShape(samples, evalTri);
      const table = fitShape(samples, (p) => evalTable(RAMP_SAMPLES, p));
      expect(tri.err, `a number with no loaded shape plays TRI (TRI ${tri.err.toFixed(3)}, table ${table.err.toFixed(3)})`).toBeLessThan(OK);
      expect(table.err, 'and not the table of the shape that is gone').toBeGreaterThan(BAD);

      // the page: the one thing only the screen shows. A CC does not redraw the WAVE label, so the gap page is captured after real knob turns from the loaded neighbour. Right after a WAVE
      // edit the header and the cell label show the value name ("----" for a gap), otherwise the label reads WAVE (stock); the graph is the TRI tile, as the tick plays TRI.
      await rig2.setWave(FIRST_SHAPE + 1);
      const step = await modPage(rig2, dest2, 'step');
      await rig2.setWave(WAVE.RAND);
      const rand = await modPage(rig2, dest2, 'rand');
      await rig2.setWave(FIRST_SHAPE + 1);
      await rig2.turnWaveTo(FIRST_SHAPE);
      await rt2.ui.wait({ ms: 300, why: REDRAW });
      const gap = await stableFrameHash(rt2);
      const gapPng = await captureRemoteOled(rt2.session, 'lfo-mod-gap-turned');
      console.info(`MOD page screen (gap-turned): ${gapPng}`);
      expect(step, 'wave 8 (STEP) shows another page than the gap').not.toBe(gap);
      expect(rand, 'RAND shows another page than the gap (its name is RND)').not.toBe(gap);
      pinFrame(`${rt2.session.options.device}.gap-page`, gap, gapPng);
      await rt2.session.expectNoFault('project with a shape number, shape module missing');
    }, 900_000);
  },
);
