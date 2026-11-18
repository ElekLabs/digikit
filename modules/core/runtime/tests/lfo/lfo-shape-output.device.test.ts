import { expect, test } from 'vitest';
import { CARD_SYMBOLS, cardRigs } from '@digi/regression/runtime/card-rig.ts';
import { bootWith, shapeWords, SHAPE_A } from './lfo-cards.ts';
import {
  evalSine,
  evalSqr,
  evalTable,
  evalTri,
  fitShape,
  FIRST_SHAPE,
  LfoRig,
  RAMP_SAMPLES,
  STOCK_EVAL,
  WAVE,
  type Fit,
} from './lfo-rig.ts';

/**
 * E6 on the device: what the LFO of track 1 outputs for a shape (docs/plans/extension-model.md, E6 "Tests"). The firmware's own output is read from the Sound array
 * in memory, not from the screen. The LFO runs into one destination word; its samples (read at fixed emulated intervals) are fitted to a shape with unknown rate and
 * start phase (`fitShape`), so a test needs no phase, speed or depth: the fit error is below `OK` for the shape that made the samples and above `BAD` for a shape that did not.
 * No SHARC: the LFO tick is a ColdFire interrupt and the zero SSI peer keeps its clock. The image is the emulator patch set (it holds `core/lfo`).
 *
 * Four cases, one boot of the runtime each:
 * - stock: `core/lfo` is in the image and no lfo service runs: waves 0 to 5 are the stock shapes (item 7);
 * - table: with `lfo.dks` and the RAMP shape loaded, WAVE reaches 7 and the output follows the shape's table, not TRI (item 1);
 * - start phase: on a trigger the shape starts at the phase of SPH (item 4);
 * - number kept: a restart with the same module keeps the wave number in `lfo.shapes` and the output follows the table again (item 5).
 */
const OK = 0.1;
const BAD = 0.14;
const READS = 100;
const GAP_MS = 12;

const describeFit = (name: string, fits: Record<string, Fit>) =>
  `${name}: ${Object.entries(fits)
    .map(([k, f]) => `${k} ${f.err.toFixed(3)}`)
    .join(', ')}`;

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
      await expect(rig.turnTo('WAVE', (WAVE.RAND + 1) << 8)).rejects.toThrow(/stops at/);
      for (const wave of [WAVE.TRI, WAVE.SINE, WAVE.SQR, WAVE.SAW, WAVE.RAMP]) {
        await rig.setWave(wave);
        await rig.tuneSpeed(dest);
        const samples = await rig.sample(dest, READS, GAP_MS);
        const fits = Object.fromEntries(
          Object.entries(STOCK_EVAL).map(([w, fn]) => [Number(w), fitShape(samples, fn)] as const),
        );
        const own = fits[wave]!;
        const note = describeFit(`wave ${wave}`, Object.fromEntries(Object.entries(fits).map(([w, f]) => [`w${w}`, f])));
        expect(own.err, `wave ${wave} follows its stock shape (${note})`).toBeLessThan(OK);
        for (const [w, f] of Object.entries(fits)) {
          const pair = [wave, Number(w)];
          // the polynomial sine and the triangle are 0.07 apart in this measure: their pair is not told apart
          if (Number(w) === wave || (pair.includes(WAVE.TRI) && pair.includes(WAVE.SINE))) continue;
          expect(f.err, `wave ${wave} is not wave ${w} (${note})`).toBeGreaterThan(own.err);
        }
      }
      await rt.session.expectNoFault('stock waves with core/lfo in the image');
    }, 900_000);

    test('table: WAVE reaches the shape number and the output follows its table, not TRI', async () => {
      const rt = await bootWith(h, 'shape', [SHAPE_A]);
      const [first] = await shapeWords(h, rt);
      expect(first! & 0xff, 'the first shape gets the first number above RAND').toBe(FIRST_SHAPE);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      await rig.setWave(FIRST_SHAPE);
      await rig.tuneSpeed(dest);
      const samples = await rig.sample(dest, READS, GAP_MS);
      const shape = fitShape(samples, (p) => evalTable(RAMP_SAMPLES, p));
      const tri = fitShape(samples, evalTri);
      const sqr = fitShape(samples, evalSqr);
      const sine = fitShape(samples, evalSine);
      const note = describeFit('fit error', { shape: shape, TRI: tri, SQR: sqr, SINE: sine });
      expect(shape.err, `the output follows the RAMP table (${note}; tolerance ${OK}: Sound words are 7 bit values and the reads are 12 ms apart)`).toBeLessThan(OK);
      expect(tri.err, `the output is not TRI (${note})`).toBeGreaterThan(BAD);
      expect(sqr.err, `the output is not SQR (${note})`).toBeGreaterThan(BAD);
      // RAND holds a value for a cycle: its samples do not fit the table
      await rig.setWave(WAVE.RAND);
      const rand = await rig.sample(dest, READS, GAP_MS);
      expect(fitShape(rand, (p) => evalTable(RAMP_SAMPLES, p)).err, 'RAND is not the table').toBeGreaterThan(BAD);
      await rt.session.expectNoFault('shape output');
    }, 900_000);

    test('start phase: on a trigger the shape starts at the phase of SPH', async () => {
      const rt = await bootWith(h, 'phase', [SHAPE_A]);
      const rig = new LfoRig(rt);
      const dest = await rig.run();
      // trigger mode, stopped: after a trigger the output is the shape at the start phase and stays there
      await rig.turnTo('MODE', 1 << 8); // FRE, TRG, HLD, ONE, HLF: 1 is TRG
      await rig.setWave(FIRST_SHAPE);
      await rig.freeze(dest);
      const out: number[] = [];
      const quarters = [0, 32, 64, 96]; // SPH in 128ths of a cycle: phase 0, 1/4, 1/2, 3/4
      for (const sph of quarters) {
        await rig.turnTo('SPH', sph << 8);
        await rt.ui.tap('TRIG 1', { until: { ms: 200, why: 'the trigger starts the LFO at its start phase (no observable state)' } });
        out.push(await rig.word(dest));
      }
      // RAMP is linear over the cycle: the four outputs rise in equal steps; the second differences stay within the quantisation of the words (and within 0x200 where the sum clamps)
      const steps = [out[1]! - out[0]!, out[2]! - out[1]!, out[3]! - out[2]!];
      const note = `outputs ${out.map((v) => v.toString(16)).join(' ')}`;
      expect(steps.every((d) => d > 0), `the output rises with SPH (${note})`).toBe(true);
      const mean = steps.reduce((a, b) => a + b, 0) / 3;
      for (const d of steps) expect(Math.abs(d - mean), `equal steps: the start phase follows SPH (${note})`).toBeLessThan(mean * 0.25 + 0x100);
      // the same SPH values on TRI give 0, +max, 0, -max: not rising
      await rig.setWave(WAVE.TRI);
      const tri: number[] = [];
      for (const sph of quarters) {
        await rig.turnTo('SPH', sph << 8);
        await rt.ui.tap('TRIG 1', { until: { ms: 200, why: 'the trigger starts the LFO at its start phase (no observable state)' } });
        tri.push(await rig.word(dest));
      }
      expect(tri[1]! - tri[0]!, 'TRI rises to its peak at a quarter cycle').toBeGreaterThan(0);
      expect(tri[3]! - tri[2]!, 'and falls after the half cycle: the shape test above is not TRI').toBeLessThan(0);
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
      await rig.tuneSpeed(dest);
      const samples = await rig.sample(dest, READS, GAP_MS);
      const shape = fitShape(samples, (p) => evalTable(RAMP_SAMPLES, p));
      expect(shape.err, `the shape answers to its number after the restart (fit ${shape.err.toFixed(3)})`).toBeLessThan(OK);
      await rt2.session.expectNoFault('wave number kept');
    }, 900_000);
  },
);

