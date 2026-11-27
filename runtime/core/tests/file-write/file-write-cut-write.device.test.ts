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
import { PATH, SIZE, cases, cutRig, oldModule, pattern, runJob } from './file-write-cut-support.ts';

// An SDK write (create, write, close) cut at four sampled card writes (setup and the host sweep: file-write-cut-support.ts).

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} SDK file write: power cuts`, () => {
    const rig = cutRig(device);

    test('the store after a cut or tear is the old or the new state, read whole by the next boot and by plusdrive', async () => {
      const { old, oldFile } = oldModule(rig, 'old.dkm');
      const mod = rig.mod('test/file-write');
      const moduleBytes = fs.readFileSync(mod.file);
      const base = await rig.card({
        name: 'cut-base',
        modules: [mod, { name: 'modules/old.dkm', file: oldFile }],
        selection: [mod.name],
      });
      const oldFiles: Files = { ...systemPackageFiles(), 'modules/old.dkm': old, [mod.name]: moduleBytes };
      const data = pattern(SIZE);
      for (const [k, [label, knobs, committed]] of cases.entries()) {
        const dk = await rig.pausedBoot(base);
        for (const [name, value] of Object.entries(knobs)) await pokeKnob(rig.session, rig.plan, name, value);
        const done = await runJob(rig, dk, SIZE, PATH);
        const writes = await word(rig.session, planSymbol(rig.plan, 'DKRT_UPLOAD_WRITES'));
        expect(done.job === 0, `${label}: the job is refused a write (status ${done.job}, ${writes} card writes counted)`).toBe(false);
        const card = await rig.after(`sdkcut${k}`);

        const want: Files = committed ? { ...oldFiles, [PATH]: data } : oldFiles;
        const next = await rig.pausedBoot(card);
        expect(await deviceListing(next), `${label}: the next boot reads the store, whole`).toEqual(listing(want));
        if (committed) {
          const view = await rig.view(`sdkcut${k}-v`);
          expect(listing(view.files), `${label}: plusdrive reads the same from the card ${view.head}`).toEqual(listing(want));
          expect(await next.verify(PATH), `${label}: VERIFY of the committed file`).toBe(sha(data));
        }
      }
    }, 3_600_000);
  });
}
