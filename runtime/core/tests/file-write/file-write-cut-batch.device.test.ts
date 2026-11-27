import fs from 'node:fs';
import { describe, expect, test } from 'vitest';
import { sha } from '@digi/regression/runtime/upload-bytes.ts';
import { listing, type Files } from '@digi/regression/runtime/card-store.ts';
import { pokeKnob } from '@digi/regression/runtime/guest-mailbox.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { systemPackageFiles } from '@digi/regression/runtime/system-package.ts';
import { deviceListing } from '@digi/regression/runtime/upload-kit.ts';
import { BATCH_N, BATCH_SIZE, batchPath, cutRig, oldModule, runBatch, shifted } from './file-write-cut-support.ts';

// An N-file batch commit cut at sampled card writes: the old store, or the new one with all N files (setup and the host sweep: file-write-cut-support.ts).

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} SDK file batch: power cuts`, () => {
    const rig = cutRig(device);

    test('a cut or tear of a batch commit leaves the old store or the new one with all files; before the commit the space of the pending files is free', async () => {
      const { old, oldFile } = oldModule(rig, 'old-b.dkm');
      const mod = rig.mod('test/file-write');
      const moduleBytes = fs.readFileSync(mod.file);
      const base = await rig.card({ name: 'cut-batch-base', modules: [mod, { name: 'modules/old.dkm', file: oldFile }], selection: [mod.name] });
      const oldFiles: Files = { ...systemPackageFiles(), 'modules/old.dkm': old, [mod.name]: moduleBytes };
      const newFiles: Files = { ...oldFiles };
      for (let i = 0; i < BATCH_N; i++) newFiles[batchPath(i)] = shifted(BATCH_SIZE, i);

      // no cut: the number of card writes W of the whole job (the repair of header A, the data writes, the table, header B, header A)
      const first = await rig.pausedBoot(base);
      const usedBefore = (await first.capacity()).STORE!.used;
      const ref = await runBatch(rig, first);
      expect([ref.job, ref.commit], 'no cut: the job and its commit end ok').toEqual([0, 0]);
      const W = await word(rig.session, planSymbol(rig.plan, 'DKRT_UPLOAD_WRITES'));
      expect(W, 'at least the repair, N data writes, a table write and two headers').toBeGreaterThanOrEqual(BATCH_N + 4);
      const refCard = await rig.after('batch-ref');
      expect(await deviceListing(await rig.pausedBoot(refCard)), 'the whole batch is in the store').toEqual(listing(newFiles));

      // sampled points: the first write, half way, before the table, a tear of header B, and a cut after header B (header A refused)
      const points: [string, Record<string, number>, boolean][] = [
        [`a cut after write 1 of ${W}`, { DKRT_UPLOAD_CUT: 1 }, false],
        [`a cut after write ${Math.floor(W / 2)} of ${W}`, { DKRT_UPLOAD_CUT: Math.floor(W / 2) }, false],
        [`a cut after write ${W - 2} of ${W}`, { DKRT_UPLOAD_CUT: W - 2 }, false],
        [`a tear of header B (write ${W - 1})`, { DKRT_UPLOAD_TEAR: W - 1 }, false],
        [`a cut after header B (write ${W - 1})`, { DKRT_UPLOAD_CUT: W - 1 }, true],
      ];
      for (const [k, [label, knobs, committed]] of points.entries()) {
        const dk = await rig.pausedBoot(base);
        for (const [name, value] of Object.entries(knobs)) await pokeKnob(rig.session, rig.plan, name, value);
        const done = await runBatch(rig, dk);
        expect(done.job === 0, `${label}: the job is refused a write (status ${done.job})`).toBe(false);
        const card = await rig.after(`batchcut${k}`);

        const want = committed ? newFiles : oldFiles;
        const next = await rig.pausedBoot(card);
        expect(await deviceListing(next), `${label}: the next boot reads the old store, or the new one with all ${BATCH_N} files`).toEqual(listing(want));
        if (!committed) {
          expect((await next.capacity()).STORE!.used, `${label}: the space of the pending files is free again`).toBe(usedBefore);
        } else {
          const view = await rig.view(`batchcut${k}-v`);
          expect(listing(view.files), `${label}: plusdrive reads the same from the card ${view.head}`).toEqual(listing(want));
          for (let i = 0; i < BATCH_N; i++) expect(await next.verify(batchPath(i)), `${label}: file ${i} verifies`).toBe(sha(shifted(BATCH_SIZE, i)));
        }
      }
    }, 3_600_000);
  });
}
