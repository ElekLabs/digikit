import { BULK_STATUS } from '@digi/dk-session/gen_bulk';
import { BulkDeviceError } from '@digi/dk-session';
import { expect, test } from 'vitest';
import { pokeKnob } from '@digi/regression/runtime/guest-mailbox.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { ufSuite, SAMPLES, pattern, hashU32, hex } from './usb-fast-support.ts';

/** A power cut in the middle of a bulk upload (the firmware's card writes cut by the store writer's knob): the old file or the new one, whole, and the next upload works. */

ufSuite('power cut', ({ dt2, rig, rp, baseCard, state, expectFile, unchanged, connect }) => {
  test('a power cut in the middle of a bulk upload leaves the old file or the new one, whole, and the next upload works', async () => {
    const data = pattern(50_000, 31);
    // DN2: the number of card writes of one upload, from a boot of its own (the last write is header A, the one before it header B)
    let writes = 0;
    if (!dt2) {
      const dk = await rig.pausedBoot(await baseCard());
      const bulk = await connect(dk);
      await bulk.session.put(SAMPLES, rp('Cut/cut.bin'), data, { expected: hashU32(data, dt2) });
      writes = await word(rig.session, planSymbol(rig.plan, 'DKRT_UPLOAD_WRITES'));
      expect(writes, 'card writes of an upload').toBeGreaterThan(3);
    }
    // [label, knobs, replace, the new file is at the destination afterwards, the upload is refused with E_WRITE]
    const cases: [string, Record<string, number>, boolean, boolean, boolean][] = dt2
      ? [
          ['mid-data of a new file', { DKRT_PD_CUT: 1 }, false, false, true],
          ['mid-data of a replace', { DKRT_PD_CUT: 1 }, true, false, true],
          ['a replace between the delete of the destination and the rename', { DKRT_PD_CUT: 4 }, true, true, true],
        ]
      : [
          ['no cut', {}, true, true, false],
          ['a cut after the first card write', { DKRT_UPLOAD_CUT: 1 }, false, false, true],
          ['a tear of header B', { DKRT_UPLOAD_TEAR: writes - 1 }, true, false, true],
          ['a cut after header B (header A is refused)', { DKRT_UPLOAD_CUT: writes - 1 }, true, true, true],
        ];
    for (const [k, [label, knobs, replace, fresh, refused]] of cases.entries()) {
      const target = replace ? 'old.bin' : 'Cut/cut.bin';
      const dk = await rig.pausedBoot(await baseCard());
      const bulk = await connect(dk, { timeoutMs: 20_000 });
      const before = (await state(dk, `pc${k}a`)).files;
      for (const [name, value] of Object.entries(knobs)) await pokeKnob(rig.session, rig.plan, name, value);
      const r = await bulk.session
        .put(SAMPLES, rp(target), data, { replace, expected: hashU32(data, dt2) })
        .catch((e) => e);
      if (refused) {
        expect(r, `${label}: the upload fails`).toBeInstanceOf(BulkDeviceError);
        expect((r as BulkDeviceError).code, `${label}: with the firmware's write error`).toBe(BULK_STATUS.E_WRITE);
      } else expect(r, `${label}: no cut: the upload works`).toBe(hashU32(data, dt2));
      const cutCard = await rig.after(`pc${k}b`);
      const next = await rig.pausedBoot(cutCard);
      const bulk2 = await connect(next);
      const s = await state(next, `pc${k}c`);
      if (fresh) await expectFile(next, s, target, data, `${label}: the new file`);
      else if (replace) expect(s.files.get(target), `${label}: the old file`).toEqual(before.get(target));
      else if (dt2) {
        // a new file is written at its final name (core/plusdrive-write, no boot cleanup): at most that one file stays, without its hash
        const left = s.files.get(target);
        if (left) expect(left.hash, `${label}: the partial file is unhashed`).not.toBe(hex(data, true).slice(0, 8));
      } else expect(s.files.has(target), `${label}: nothing at the destination`).toBe(false);
      unchanged(before, s.files, [target, 'Cut/cut.bin']);
      // the next upload works, and the destination is as before it
      const more = pattern(9000, 41);
      await bulk2.session.put(SAMPLES, rp('after.bin'), more, { expected: hashU32(more, dt2) });
      const t = await state(next, `pc${k}d`);
      await expectFile(next, t, 'after.bin', more, `${label}: the next upload`);
      if (dt2 && !replace && !fresh) {
        // the next upload of the same name replaces the partial file and verifies
        await bulk2.session.put(SAMPLES, rp(target), data, { replace: true, expected: hashU32(data, dt2) });
        await expectFile(next, await state(next, `pc${k}e`), target, data, `${label}: the re-upload`);
      } else expect(t.files.get(target), `${label}: the destination is unchanged by it`).toEqual(s.files.get(target));
    }
  }, 3_600_000);
});
