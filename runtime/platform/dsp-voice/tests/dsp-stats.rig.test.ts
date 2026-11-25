/**
 * The machine spans of the DSP stats window (docs/plans/profiling.md 13.1, P8d) in the dispatch code of the machines service, on the SHARC-only rig, DT2 and DN2.
 *
 * The window is the one core/dsp-link writes the address of into `_dv_diag` word 31 when stats are on (the rig pokes it). The rig runs no block tail, so the
 * rows hold the running sums of the spans: calls, cycles (the emulator's cycle counter is instructions), the largest call, and the sum of the block (word 5).
 * What the tail folds is tested with core/dsp-link (runtime/platform/dsp-link/tests/dsp-stats.rig.test.ts).
 */
import { describe, expect, it } from 'vitest';
import { MachineRig, buildMachine, machineSpecs, nw, type MachineBlock } from '@digi/sharc-rig-kit';

const WINDOW = 0x200fc000;
const WINDOW_WORDS = 104;
const PTR_WORD = 31; // `_dv_diag` word 31
const HEAD = 4;
const ROW = 6;
const BLOCKS = 4;
const row = (w: readonly (number | null)[], r: number) => w.slice(HEAD + ROW * r, HEAD + ROW * (r + 1)).map((x) => x ?? 0);

async function play(
  spec: ReturnType<typeof machineSpecs>[number],
  stats: boolean,
  withoutSpans = false,
): Promise<MachineBlock[]> {
  const rig = await MachineRig.open(spec, { built: buildMachine(spec, withoutSpans) });
  try {
    const diag = rig.built.data._dv_diag!;
    rig.poke(nw(diag) + PTR_WORD, stats ? WINDOW : 0);
    for (let k = 0; k < WINDOW_WORDS; k++) rig.poke(nw(WINDOW) + k, 0); // what a clear generation does
    rig.setParam(spec.params[0]!.id, spec.params[0]!.default);
    const out: MachineBlock[] = [];
    for (let b = 0; b < BLOCKS; b++) {
      rig.trigger();
      out.push(await rig.block('friendly', [[WINDOW, WINDOW_WORDS]]));
    }
    return out;
  } finally {
    await rig.close();
  }
}

for (const device of ['dt2', 'dn2'] as const) {
  const spec = machineSpecs().find((s) => s.device === device && s.noteOn !== null)!;
  describe(`machine spans on ${device.toUpperCase()}`, () => {
    it('stats on: the machine row counts the module calls (note_on, render) with their cycles, largest call and block sum', async () => {
      const blocks = await play(spec, true);
      for (const b of blocks) expect(b.returned && b.contextBad.length === 0, b.halt).toBe(true);
      // the module calls of each block: what the marks cost with stats off, 6 instructions a call, says how many there were
      const off = await play(spec, false);
      const base = await play(spec, false, true);
      const spans = off.map((b, k) => (b.instructions - base[k]!.instructions) / 6);
      const w = blocks.at(-1)!.extra[0]!;
      const [calls, lo, hi, largest, , sum] = row(w, 0);
      expect(calls, 'one count per module call').toBe(spans.reduce((x, y) => x + y, 0));
      expect(calls).toBeGreaterThan(0);
      expect(hi).toBe(0);
      let before = 0;
      let seen = 0;
      blocks.forEach((b, k) => {
        const at = row(b.extra[0]!, 0)[1]!;
        if (spans[k] === 0) expect(at, `block ${k} calls no module: no span`).toBe(before);
        else {
          expect(at - before, `the spans of block ${k} are part of the block`).toBeGreaterThan(0);
          expect(at - before, `the spans of block ${k} are part of the block`).toBeLessThan(b.instructions);
        }
        before = at;
        seen += spans[k]!;
        expect(row(b.extra[0]!, 0)[0], `calls after block ${k}`).toBe(seen);
      });
      expect(largest! * calls!, 'the largest call is at least the mean').toBeGreaterThanOrEqual(lo!);
      expect(largest!, 'and at most the sum').toBeLessThanOrEqual(lo!);
      expect(sum, 'the rig runs no block tail: the block sum is the running sum').toBe(lo);
      for (let r = 1; r < 16; r++) expect(row(w, r), `row ${r} untouched`).toEqual([0, 0, 0, 0, 0, 0]);
      expect(w.slice(0, 3), "the header is the tail's").toEqual([0, 0, 0]);
    });

    it('the samples are the same with stats on and off', async () => {
      const on = await play(spec, true);
      const off = await play(spec, false);
      expect(on.map((b) => b.samples)).toEqual(off.map((b) => b.samples));
    });

    it('stats off: the marks cost 6 instructions per module call (note_on, render) and write nothing', async () => {
      const off = await play(spec, false);
      const base = await play(spec, false, true);
      const extra = off.map((b, k) => b.instructions - base[k]!.instructions);
      for (const n of extra) expect(n % 6, 'whole marks only').toBe(0);
      expect(extra.reduce((x, y) => x + y, 0), 'some module calls ran').toBeGreaterThan(0);
      expect(off.at(-1)!.extra[0]!.every((x) => (x ?? 0) === 0)).toBe(true);
    });

    it('stats on: the added instructions per call stay under 100', async () => {
      const on = await play(spec, true);
      const off = await play(spec, false);
      const base = await play(spec, false, true);
      const added = on.map((b, k) => b.instructions - off[k]!.instructions);
      const spans = off.map((b, k) => (b.instructions - base[k]!.instructions) / 6);
      console.log(`${device}: stats on adds ${added.join(', ')} instructions in blocks of ${spans.join(', ')} spans (off adds 6 per span)`);
      added.forEach((n, k) => expect(n).toBeLessThanOrEqual(100 * spans[k]!));
    });
  });
}
