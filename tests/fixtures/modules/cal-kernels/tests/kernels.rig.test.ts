// test/cal-kernels on the SHARC-only rig: every kernel runs the instruction count its header states in every render call, whatever the
// note, the velocity or the block, and writes finite samples. The count is the sum of the per-pc counts inside the module's own code,
// so core/dsp-voice around the call is not in it (the debug module's spans around the module's render, which the calibration profile
// `kernels` reads, count the same range).
import { describe, expect, it } from 'vitest';
import { DEVICES, MachineRig, machineSpec } from '@digi/sharc-rig-kit';

/** Instructions of one render call, from the first instruction to the JUMP back (see src/dsp/cal_kernels.s). */
const COUNTS: Record<string, number> = {
  alu: 20_050,
  dm: 19_959,
  mem: 19_244,
  branch: 20_046,
  mix: 19_956,
};

const MODULE = 'test/cal-kernels';

describe.each(DEVICES)('%s', (device) => {
  describe.each(Object.entries(COUNTS))('kernel %s', (id, expected) => {
    it('runs the same instruction count in every steady block, and writes finite samples', async () => {
      const spec = machineSpec(`${MODULE}#${id}`, device);
      expect(spec).not.toBeNull();
      const rig = await MachineRig.open(spec!, { profile: true });
      try {
        const [lo, hi] = [rig.built.moduleCode[0]![0], rig.built.moduleCode.at(-1)![1]];
        const steady: number[] = [];
        // two notes (note-on block, then steady blocks), different velocities
        for (const [note, vel] of [
          [36, 20],
          [84, 127],
        ] as const) {
          rig.setNote(note, vel);
          rig.trigger();
          for (let k = 0; k < 6; k++) {
            const b = await rig.block('friendly');
            expect(b.returned, `${id} block ${k} returned`).toBe(true);
            expect(b.faults, `${id} block ${k} faults`).toBe(0);
            expect(b.samples.every(Number.isFinite), `${id} block ${k} finite`).toBe(true);
            expect(Math.max(...b.samples.map(Math.abs)), `${id} block ${k} level`).toBeLessThan(1e-3);
            // blocks 0 and 1 of a note include its note_on (and, on the Digitone II, block 0 runs no module code)
            if (k >= 2) steady.push(b.profile.reduce((n, [pc, c]) => (pc >= lo && pc < hi ? n + c : n), 0));
          }
        }
        expect(steady, id).toEqual(steady.map(() => expected));
        expect(steady.length).toBe(8);
      } finally {
        await rig.close();
      }
    }, 120_000);
  });
});
