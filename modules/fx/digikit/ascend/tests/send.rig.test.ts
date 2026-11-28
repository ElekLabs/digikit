import { fileURLToPath } from 'node:url';
import { FxRig, P0, f2w } from '@digi/sharc-rig-kit';
import { expect, test } from 'vitest';

const sources = [fileURLToPath(new URL('../src/dsp/ascend.s', import.meta.url))];
const frames = 32;
const tracks = Array.from({ length: 16 }, (_, track) => track);
const zero = Array<number>(2 * frames).fill(0);

for (const device of ['dt2', 'dn2'] as const) {
  test(`${device}: the notch survives a reversal on the actual send hook`, async () => {
    const rig = await FxRig.open(
      [{ pc: '_ascend_process', state: '', desc: '_ascend_desc', params: [0.25, 63, 0.5, 1].map(f2w) }],
      { device, sources },
    );
    // The stimulus repeats every block at 1500 Hz. Different channel levels
    // let the same check detect accidentally sharing the two filter histories.
    const input = zero.map((_, i) => (i < frames ? 0.5 : 0.3) * Math.sin((2 * Math.PI * (i % frames)) / frames));
    let earlierPeak = 0;
    let notchPeak = 0;
    try {
      rig.setSlotmap(tracks);
      rig.setAmounts(0, [0, 0, 127]);
      for (let block = 0; block < 2384; block++) {
        if (block === 2320) {
          rig.setRowWord(0, P0, f2w(0.3));
          rig.setRowWord(0, P0 + 1, f2w(-64));
        }
        if (block === 2352) rig.setRowWord(0, P0 + 3, f2w(0));
        rig.setTrack(2, input);
        rig.setReturn(zero);
        const result = await rig.sendBlock(input, 'friendly');
        if (!result.returned || result.faults[0] || result.ctxBad.length || result.regsBad.length)
          throw new Error(`block ${block}: send hook failed (${result.halt}, ${result.faults[0]} faults)`);

        const peak = Math.max(...result.master.map((value, i) => Math.abs(value + result.ret[i]!)));
        if (block === 1500) earlierPeak = peak;
        // The fixed all-pass phase places the 1500 Hz notch at 0.386 turns,
        // near block 2320. A reset during reversal would restore a loud tone.
        if (block >= 2288 && block < 2352) notchPeak = Math.max(notchPeak, peak);
        if (block > 2352) expect(result.ret.every((value) => value === 0)).toBe(true);
      }
      expect(earlierPeak).toBeGreaterThan(0.18);
      expect(notchPeak).toBeLessThan(0.015);
    } finally {
      await rig.close();
    }
  });
}
