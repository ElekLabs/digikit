import { expect, test } from 'vitest';
import { pollUntil } from '@digi/test-support';
import { CARD_SYMBOLS } from '@digi/regression/runtime/card-symbols.ts';
import { cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { sendCc } from '@digi/regression/runtime/midi.ts';
import type { TestRuntime } from '@digi/regression/test-runtime.ts';
import { bootWith, shapeWords, SHAPE_A } from './lfo-cards.ts';
import {
  BAD,
  evalSine,
  evalSqr,
  evalTable,
  evalTri,
  describeSamples,
  fitShape,
  FIRST_SHAPE,
  KNOB_WORD,
  LfoRig,
  OK,
  PAIR_MARGIN,
  RAMP_SAMPLES,
  SINE_OK,
  STOCK_EVAL,
  WAVE,
  type Fit,
} from './lfo-rig.ts';

/**
 * E6 on the device: what the LFO of track 1 outputs for a shape (docs/plans/extension-model.md, E6 "Tests"). The firmware's own output is read from the Sound array
 * in memory, not from the screen. The LFO runs into one destination word; its samples (read at fixed emulated intervals) are fitted to a shape with unknown rate and
 * start phase (`fitShape`), so a test needs no phase, speed or depth: the fit error is below `OK` for the shape that made the samples and above `BAD` for a shape that did not.
 * No SHARC: the LFO tick is a ColdFire interrupt (vector 191) that the zero-DSP session paces itself (`enable_zero_dsp_timing`: the SSI0 request chain at 96 kHz forces the
 * vector from the DMA 50 completion), so it runs at 1500 Hz. The image is the emulator patch set; the lfo service installs its hooks when it loads.
 *
 * Five cases, one boot of the runtime each (the last takes over the runtime of the one before it):
 * - stock: no lfo service runs: waves 0 to 5 are the stock shapes (item 7);
 * - table: with `lfo.dks` and the RAMP shape loaded, WAVE reaches 7 and the output follows the shape's table, not TRI (item 1);
 * - start phase: on a note in MODE TRG the shape starts at the phase of SPH (item 4); each note takes a voice slot (with voice rotation the next slot, with one voice per track slot 0), whose phase is checked exactly;
 * - number kept: a restart with the same module keeps the wave number in `lfo.shapes` and the output follows the table again (item 5);
 * - reload: `RELOAD services/lfo.dks` on that runtime answers OK, the shape table is republished and the output follows it again (the order of the steps and the refusals are host tests).
 * The hook install and the unload (MODULES and CAPACITY as before) are shown by the table case here and by `module-load`; the refusals are assembly tests.
 */
const READS = 100;
const GAP_MS = 12;

const describeFit = (name: string, fits: Record<string, Fit>) =>
  `${name}: ${Object.entries(fits)
    .map(([k, f]) => `${k} ${f.err.toFixed(3)}`)
    .join(', ')}`;

/** The runtime that the 'number kept' case leaves running, for the reload case that follows it. */
let kept: TestRuntime | undefined;

cardRigs(
  'lfo shapes: output',
  { dsp: 'zero', symbols: CARD_SYMBOLS, initial: () => ({}), uploads: true, relaxFs: true },
  (h) => {
    test('stock: core/lfo in the image and no lfo service, waves TRI, SINE, SQR, SAW and RAMP run as before', async () => {
      const rt = await h.first();
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      // WAVE stops at RAND: the maximum of the parameter is the stock one while no service is loaded
      await rig.setWave(WAVE.RAND);
      // O8: the MIDI CC path stops at the stock maximum as well
      await rig.setWave(2);
      await sendCc(rt, 1, 106, FIRST_SHAPE);
      await pollUntil(rt.clock, () => rig.word(KNOB_WORD.WAVE), (got) => got === WAVE.RAND << 8, {
        withinMs: 1000,
        intervalMs: 20,
        message: 'CC 106 = 7 stores the stock maximum RAND without a shape',
      });
      for (const wave of [WAVE.TRI, WAVE.SINE, WAVE.SQR, WAVE.SAW, WAVE.RAMP]) {
        await rig.setWave(wave);
        const samples = await rig.sample(dest, READS, GAP_MS);
        const fits = Object.fromEntries(
          Object.entries(STOCK_EVAL).map(([w, fn]) => [Number(w), fitShape(samples, fn)] as const),
        );
        const own = fits[wave]!;
        const note = describeFit(`wave ${wave}`, Object.fromEntries(Object.entries(fits).map(([w, f]) => [`w${w}`, f])));
        expect(own.err, `wave ${wave} follows its stock shape (${note})`).toBeLessThan(wave === WAVE.SINE ? SINE_OK : OK);
        for (const [w, f] of Object.entries(fits)) {
          if (Number(w) === wave) continue;
          // the true sine against the triangle is the closest pair (about 0.07 apart in this measure, finding of lfo-fit.test.ts): it needs the margin, the others only to be worse
          const close = [wave, Number(w)].includes(WAVE.TRI) && [wave, Number(w)].includes(WAVE.SINE);
          expect(f.err, `wave ${wave} is not wave ${w} (${note})`).toBeGreaterThan(own.err + (close ? PAIR_MARGIN : 0));
        }
      }
      await rt.session.expectNoFault('stock waves with core/lfo in the image');
    }, 900_000);

    test('table: WAVE reaches the shape number and the output follows its table, not TRI', async () => {
      const rt = await bootWith(h, 'shape', [SHAPE_A]);
      const [first] = await shapeWords(h, rt);
      expect(first! & 0xff, 'the first shape gets the first number above RAND').toBe(FIRST_SHAPE);
      // the hooks installed into the real image and the fixture owns no machine type
      expect((await h.usb(rt, (dk) => dk.modules())).map((m) => [m.name, m.types]), 'MODULES lists the shape and it owns no machine type').toEqual([[SHAPE_A, []]]);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      await rig.setWave(FIRST_SHAPE);
      const samples = await rig.sample(dest, READS, GAP_MS);
      const shape = fitShape(samples, (p) => evalTable(RAMP_SAMPLES, p));
      const tri = fitShape(samples, evalTri);
      const sqr = fitShape(samples, evalSqr);
      const sine = fitShape(samples, evalSine);
      const note = describeFit('fit error', { shape: shape, TRI: tri, SQR: sqr, SINE: sine });
      console.info(`shape output ${h.device}: ${note}`);
      expect(shape.err, `the output follows the RAMP table (${note}; tolerance ${OK}: the reads are 12 ms apart and jitter by the 0.67 ms tick; samples ${describeSamples(samples)})`).toBeLessThan(OK);
      expect(tri.err, `the output is not TRI (${note})`).toBeGreaterThan(BAD);
      expect(sqr.err, `the output is not SQR (${note})`).toBeGreaterThan(BAD);
      // RAND holds a value for a cycle: its samples do not fit the table
      await rig.setWave(WAVE.RAND);
      const rand = await rig.sample(dest, READS, GAP_MS);
      expect(fitShape(rand, (p) => evalTable(RAMP_SAMPLES, p)).err, 'RAND is not the table').toBeGreaterThan(BAD);
      // O8: with a shape loaded the MIDI CC path reaches the shape numbers
      await sendCc(rt, 1, 106, FIRST_SHAPE);
      await pollUntil(rt.clock, () => rig.word(KNOB_WORD.WAVE), (got) => got === FIRST_SHAPE << 8, {
        withinMs: 1000,
        intervalMs: 20,
        message: 'CC 106 = 7 stores WAVE 0x0700 with a shape loaded',
      });
      await rt.session.expectNoFault('shape output');
    }, 900_000);

    test('start phase: on a trigger the shape starts at the phase of SPH', async () => {
      const rt = await bootWith(h, 'phase', [SHAPE_A]);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      // trigger mode, stopped: after a trigger the output is the shape at the start phase and stays there
      await rig.set('MODE', 1); // FRE, TRG, HLD, ONE, HLF: 1 is TRG
      await rig.setWave(FIRST_SHAPE);
      await rig.freeze(dest);
      const quarters = [0, 32, 64, 96]; // SPH in 128ths of a cycle: phase 0, 1/4, 1/2, 3/4
      // a note takes a voice slot (with voice rotation the next slot, with one voice per track slot 0): the phase of that slot is the exact start phase, and its frame word is the output
      const triggered = async (): Promise<number[]> => {
        const outs: number[] = [];
        for (const sph of quarters) {
          await rig.set('SPH', sph);
          const slot = await rig.trigger(LfoRig.startPhase(sph));
          expect(await rig.phase(slot), `the start phase of voice slot ${slot} at SPH ${sph}`).toBe(LfoRig.startPhase(sph));
          outs.push(await rig.live(dest, slot));
        }
        return outs;
      };
      const out = await triggered();
      // RAMP is linear over the cycle: the four outputs move in equal steps, all in one direction (which one depends on how SPH maps to the start phase [O]). The
      // outputs are read from the DSP frame word, which has no ripple, so the steps match within a few units. A shape that ignored SPH would give four equal outputs.
      const steps = [out[1]! - out[0]!, out[2]! - out[1]!, out[3]! - out[2]!];
      const note = `outputs ${out.map((v) => v.toString(16)).join(' ')}`;
      expect(steps.every((d) => d > 0) || steps.every((d) => d < 0), `the output moves one way with SPH (${note})`).toBe(true);
      const mags = steps.map((d) => Math.abs(d));
      expect(Math.min(...mags), `every step is at least 0x40 (${note})`).toBeGreaterThanOrEqual(0x40);
      expect(Math.max(...mags) - Math.min(...mags), `equal steps: the start phase follows SPH (${note})`).toBeLessThanOrEqual(4);
      // the same SPH values on TRI give 0, +max, 0, -max: the first step and the last step have opposite signs
      await rig.setWave(WAVE.TRI);
      const tri = await triggered();
      const triNote = `TRI outputs ${tri.map((v) => v.toString(16)).join(' ')}`;
      expect((tri[1]! - tri[0]!) * (tri[3]! - tri[2]!), `TRI rises and falls over the same quarters: its first and last step differ in sign (${triNote})`).toBeLessThan(0);
      await rt.session.expectNoFault('shape start phase');
    }, 900_000);

    test('number kept: a restart with the same module keeps the wave number in lfo.shapes', async () => {
      const rt = await bootWith(h, 'kept', [SHAPE_A]);
      const before = await shapeWords(h, rt);
      expect(before[0]! & 0xff, 'the shape has its number').toBe(FIRST_SHAPE);
      expect(before[0]! >>> 8, 'with the hash of its key').not.toBe(0);
      const rt2 = await h.restart(rt);
      const after = await shapeWords(h, rt2);
      expect(after, 'the sticky map is the same after the restart').toEqual(before);
      const rig = new LfoRig(rt2);
      const dest = await rig.run();
      await rig.setWave(FIRST_SHAPE);
      const samples = await rig.sample(dest, READS, GAP_MS);
      const shape = fitShape(samples, (p) => evalTable(RAMP_SAMPLES, p));
      expect(shape.err, `the shape answers to its number after the restart (fit ${shape.err.toFixed(3)}; samples ${describeSamples(samples)})`).toBeLessThan(OK);
      await rt2.session.expectNoFault('wave number kept');
      kept = rt2;
    }, 900_000);

    test('reload: RELOAD services/lfo.dks answers OK and the output follows the shape table again', async () => {
      const rt = kept;
      if (rt === undefined) throw new Error("the 'number kept' case runs first and leaves the runtime");
      const before = await shapeWords(h, rt);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      await rig.setWave(FIRST_SHAPE);
      const waveBefore = await rig.word(KNOB_WORD.WAVE);
      const end = await h.usb(rt, (dk) => dk.reloadAndWait('services/lfo.dks'));
      expect(end.result, 'the reload of the lfo service answers OK').toBe(0);
      expect(await rig.word(KNOB_WORD.WAVE), 'the WAVE knob word is unchanged').toBe(waveBefore);
      const samples = await rig.sample(dest, READS, GAP_MS);
      const shape = fitShape(samples, (p) => evalTable(RAMP_SAMPLES, p));
      expect(shape.err, `after: the output follows the RAMP table (fit ${shape.err.toFixed(3)}; samples ${describeSamples(samples)})`).toBeLessThan(OK);
      expect(await shapeWords(h, rt), 'the sticky map holds the same wave numbers').toEqual(before);
      await rt.session.expectNoFault('lfo reload');
    }, 900_000);
  },
);

