import { expect, test } from 'vitest';
import { pokeKnob } from '@digi/regression/runtime/card-kit.ts';
import { SETTLE } from '@digi/regression/runtime/fx-live.ts';
import { pdwSuite, pattern, slot4, cat, staged, noStagingFolder, WRITE, REPLACE, SAMPLES, E_EXISTS } from './plusdrive-write-support.ts';

// +Drive writes through core/plusdrive-write, power cuts and recovery (docs/plans/usb-fast.md 3.1, finding 120; setup: plusdrive-write-support.ts).

pdwSuite('+Drive writes through core/plusdrive-write: power cuts and recovery', { devices: ['dt2'] }, (t) => {
  const { rig, settle, baseCard, run, snapshot, same } = t;
  // A cut at each step of a replace (the adapter stops dead, as a power cut would; DKRT_PD_CUT). After the reboot the destination holds the old
  // file or the new one, whole, never nothing and never a partial file, the staging folder is cleaned, and no other file changed. A new file is
  // written at its name, not staged: its cuts are in the direct-write test below.
  const cuts: [string, number, boolean][] = [
    // [label, step, new file at the destination afterwards]
    ['mid-data of a replace', 1, false],
    ['a replace hashed, before the journal', 2, false],
    ['a replace with its journal written, before the delete', 3, false],
    ['a replace between the delete of the destination and the rename', 4, true],
    ['a replace renamed, before the journal is deleted', 5, true],
    ['a replace under two names (a cut inside the rename, with its journal)', 6, true],
  ];
  test('a power cut at any step of a replace leaves the old file or the new one, and the next boot cleans the staging folder', async () => {
    const data = pattern(50000);
    for (const [k, [label, step, fresh]] of cuts.entries()) {
      const target = 'Samples/old.bin';
      const dk = await rig.pausedBoot(await baseCard());
      await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
      const before = (await snapshot(`pdc${k}a`)).files;
      await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', step);
      const r = await run(dk, WRITE | SAMPLES | REPLACE, data.length, target);
      expect(r.job, `${label}: the job ends with a write error (the cut)`).toBe(-57);
      const cut = (await snapshot(`pdc${k}b`)).card;
      // boot the card the cut left; the runtime's recovery runs in the UI loop
      const dk2 = await rig.pausedBoot(cut);
      await rig.runtime.ui.wait({ ms: 3000, why: SETTLE });
      const s = await snapshot(`pdc${k}c`);
      const here = s.files.get(target);
      if (fresh) {
        expect(here, `${label}: the new file is at the destination`).toEqual({ size: data.length + 16, hash: slot4(data) });
        expect((await cat(s.card, target)).subarray(0, data.length)).toEqual(data);
      } else {
        expect(here, `${label}: the old file is at the destination`).toEqual(before.get(target));
      }
      expect(staged(s.files), `${label}: the staging folder is empty`).toEqual([]);
      expect(noStagingFolder(s.files), `${label}: and removed`).toBe(true);
      same(before, s.files, [target]);
      // the data of the destination is still allocated: another file is written and the destination reads as before
      const more = pattern(40000);
      const m = await run(dk2, WRITE | SAMPLES, more.length, 'Samples/after.bin');
      expect([m.close, m.job], `${label}: the next write works`).toEqual([0, 0]);
      const t = await snapshot(`pdc${k}d`);
      expect(t.files.get(target), `${label}: the destination is unchanged by it`).toEqual(here);
      if (here && fresh) expect((await cat(t.card, target)).subarray(0, data.length), `${label}: its bytes`).toEqual(data);
      expect((await cat(t.card, 'Samples/after.bin')).subarray(0, more.length)).toEqual(more);
    }
  }, 3_600_000);

  test('a rename that fails after the destination was deleted is finished by the recovery without a reboot', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    const before = (await snapshot('pdr0')).files;
    const data = pattern(20000);
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', 7);
    const r = await run(dk, WRITE | REPLACE | SAMPLES, data.length, 'Samples/old.bin');
    expect(r.job, 'the caller is told: write error').toBe(-57);
    await rig.runtime.ui.wait({ ms: 3000, why: SETTLE }); // the UI pass runs the recovery again
    const s = await snapshot('pdr1');
    expect(s.files.get('Samples/old.bin'), 'the new file is at the destination').toEqual({ size: data.length + 16, hash: slot4(data) });
    expect((await cat(s.card, 'Samples/old.bin')).subarray(0, data.length)).toEqual(data);
    expect(staged(s.files)).toEqual([]);
    expect(noStagingFolder(s.files)).toBe(true);
    same(before, s.files, ['Samples/old.bin']);
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', 0);
    const again = await run(dk, WRITE | SAMPLES, 3000, 'Samples/next.bin');
    expect([again.close, again.job], 'the next file is accepted').toEqual([0, 0]);
    await rig.session.expectNoFault('rename failure');
  }, 1_800_000);

  test('an unhashed file at the destination (a stock writer that died) is a file that exists: E_EXISTS without REPLACE, replaced through the journal with it', async () => {
    const dk = await rig.pausedBoot(await baseCard());
    await rig.runtime.ui.wait({ ms: 2000, why: SETTLE });
    await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', 8);
    const r = await run(dk, WRITE | SAMPLES, 7000, 'Samples/unhashed.bin');
    expect(r.job).toBe(-57);
    const cut = (await snapshot('pdu0')).card;
    const dk2 = await rig.pausedBoot(cut);
    await rig.runtime.ui.wait({ ms: 3000, why: SETTLE });
    const before = (await snapshot('pdu1')).files;
    expect(before.has('Samples/unhashed.bin'), 'the file is on the card').toBe(true);
    const ex = await run(dk2, WRITE | SAMPLES, 777, 'Samples/unhashed.bin');
    expect([ex.create, ex.job], 'it exists').toEqual([E_EXISTS, E_EXISTS]);
    const data = pattern(6000);
    const rep = await run(dk2, WRITE | REPLACE | SAMPLES, data.length, 'Samples/unhashed.bin');
    expect([rep.close, rep.job], 'replaced').toEqual([0, 0]);
    const s = await snapshot('pdu2');
    expect(s.files.get('Samples/unhashed.bin')).toEqual({ size: data.length + 16, hash: slot4(data) });
    expect(staged(s.files)).toEqual([]);
    same(before, s.files, ['Samples/unhashed.bin']);
  }, 1_800_000);

  // A new file is written at its final name (direct write). There is no record and no boot cleanup: a cut between the creation and the close hash
  // leaves that one file, without a hash, visible at its name; nothing else changes. The next upload of the path finds a file it must replace
  // (without REPLACE the device says E_EXISTS) and replaces it through the staged write.
  test('a power cut during a direct write leaves at most that one unhashed file and nothing else changed; the next upload replaces it; a cut after the hash leaves the whole file', async () => {
    const data = pattern(50000);
    for (const [k, [label, step, whole]] of ([['mid-data', 1, false], ['hashed, before the end of the call', 2, true]] as [string, number, boolean][]).entries()) {
      const target = 'Samples/Cut/direct.bin';
      const dk = await rig.pausedBoot(await baseCard());
      await settle();
      const before = (await snapshot(`pdx${k}a`)).files;
      await pokeKnob(rig.session, rig.plan, 'DKRT_PD_CUT', step);
      const r = await run(dk, WRITE | SAMPLES, data.length, target);
      expect(r.job, `${label}: the job ends with a write error (the cut)`).toBe(-57);
      const cut = await snapshot(`pdx${k}b`);
      expect(cut.files.has(target), `${label}: the file is on the card at the cut`).toBe(true);
      expect(staged(cut.files), `${label}: no record and no staged file`).toEqual([]);
      expect(noStagingFolder(cut.files), `${label}: no staging folder`).toBe(true);
      same(before, cut.files, [target, 'Samples/Cut/']);
      const dk2 = await rig.pausedBoot(cut.card);
      await rig.runtime.ui.wait({ ms: 3000, why: SETTLE }); // the runtime's recovery runs: it does not touch the file
      const s = await snapshot(`pdx${k}c`);
      expect(s.files.has(target), `${label}: the file is still there after the boot`).toBe(true);
      if (whole) {
        expect(s.files.get(target), `${label}: the file is whole`).toEqual({ size: data.length + 16, hash: slot4(data) });
        expect((await cat(s.card, target)).subarray(0, data.length)).toEqual(data);
      }
      same(before, s.files, [target, 'Samples/Cut/']);
      expect(noStagingFolder(s.files), `${label}: still no staging folder`).toBe(true);
      // the path exists: without REPLACE the upload is refused, with REPLACE it replaces the partial file
      const again = await run(dk2, WRITE | SAMPLES, data.length, target);
      expect([again.create, again.job], `${label}: E_EXISTS without REPLACE`).toEqual([E_EXISTS, E_EXISTS]);
      const fresh = pattern(30000);
      const rep = await run(dk2, WRITE | REPLACE | SAMPLES, fresh.length, target);
      expect([rep.close, rep.job], `${label}: replaced`).toEqual([0, 0]);
      const t = await snapshot(`pdx${k}d`);
      expect(t.files.get(target), `${label}: the new file is whole`).toEqual({ size: fresh.length + 16, hash: slot4(fresh) });
      expect((await cat(t.card, target)).subarray(0, fresh.length)).toEqual(fresh);
      same(before, t.files, [target, 'Samples/Cut/']);
    }
    await rig.session.expectNoFault('direct write cut');
  }, 3_600_000);

});
