/**
 * The effect spans of the DSP stats window (docs/plans/profiling.md 13.1, P8d) in the dispatch code of the fx service, on the SHARC-only rig, DT2 and DN2.
 *
 * The window is the one core/dsp-link writes the address of into `_fx_diag` word 7 when stats are on (the rig pokes it). The rig runs no block tail: the rows
 * hold running sums (core/dsp-link folds the blocks: modules/core/adapters/dsp-link/tests/dsp-stats.rig.test.ts). Effect rows are the stats rows 8 to 15.
 */
import { describe, expect, it } from 'vitest';
import { FxRig, buildFx, nw, type FxBlock } from '@digi/sharc-rig-kit';
import { EFFECTS, RAMP } from './fx-send-model.ts';

const WINDOW = 0x200fc000;
const WINDOW_WORDS = 104;
const PTR_WORD = 7; // `_fx_diag` word 7
const HEAD = 4;
const ROW = 6;
const BLOCKS = 4;
const FX_ROW0 = 8;
const row = (w: readonly (number | null)[], r: number) => w.slice(HEAD + ROW * r, HEAD + ROW * (r + 1)).map((x) => x ?? 0);
const ADD1 = { pc: '_te_add1', state: '_te_state_a' };

async function play(device: 'dt2' | 'dn2', stats: boolean, withoutSpans = false): Promise<FxBlock[]> {
  const rig = await FxRig.open([ADD1], { sources: EFFECTS, device, built: buildFx(EFFECTS, device, withoutSpans) });
  try {
    rig.poke(nw(rig.built.data._fx_diag!) + PTR_WORD, stats ? WINDOW : 0);
    for (let k = 0; k < WINDOW_WORDS; k++) rig.poke(nw(WINDOW) + k, 0); // what a clear generation does
    const out: FxBlock[] = [];
    for (let b = 0; b < BLOCKS; b++) out.push(await rig.block(RAMP, 'friendly', {}, [[WINDOW, WINDOW_WORDS]]));
    return out;
  } finally {
    await rig.close();
  }
}

for (const device of ['dt2', 'dn2'] as const) {
  describe(`effect spans on ${device.toUpperCase()}`, () => {
    it('stats on: the effect row counts one call per block with its cycles, largest call and block sum', async () => {
      const blocks = await play(device, true);
      for (const b of blocks) expect(b.returned && b.ctxBad.length === 0 && b.regsBad.length === 0, b.halt).toBe(true);
      const w = blocks.at(-1)!.extra[0]!;
      const [calls, lo, hi, largest, , sum] = row(w, FX_ROW0);
      expect(calls, 'one effect, one call per block').toBe(BLOCKS);
      expect(hi).toBe(0);
      let before = 0;
      blocks.forEach((b, k) => {
        const at = row(b.extra[0]!, FX_ROW0)[1]!;
        expect(at - before, `the span of block ${k}`).toBeGreaterThan(0);
        expect(at - before, `the span of block ${k} is part of the block`).toBeLessThan(b.instructions);
        before = at;
      });
      expect(largest! * BLOCKS).toBeGreaterThanOrEqual(lo!);
      expect(largest!).toBeLessThanOrEqual(lo!);
      expect(sum).toBe(lo);
      for (let r = 0; r < 16; r++) if (r !== FX_ROW0) expect(row(w, r), `row ${r} untouched`).toEqual([0, 0, 0, 0, 0, 0]);
    });

    it('the master block is the same with stats on and off', async () => {
      const on = await play(device, true);
      const off = await play(device, false);
      expect(on.map((b) => b.master)).toEqual(off.map((b) => b.master));
    });

    it('stats off: the marks cost 6 instructions per call and write nothing', async () => {
      const off = await play(device, false);
      const base = await play(device, false, true);
      expect(off.map((b, k) => b.instructions - base[k]!.instructions)).toEqual(Array(BLOCKS).fill(6));
      expect(off.at(-1)!.extra[0]!.every((x) => (x ?? 0) === 0)).toBe(true);
    });

    it('stats on: the added instructions per call stay under 100', async () => {
      const on = await play(device, true);
      const off = await play(device, false);
      const added = on.map((b, k) => b.instructions - off[k]!.instructions);
      console.log(`${device}: stats on adds ${added.join(', ')} instructions per call (off adds 6)`);
      for (const n of added) expect(n).toBeLessThanOrEqual(100);
    });
  });
}
