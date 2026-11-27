import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { sha } from '@digi/regression/runtime/upload-bytes.ts';
import { listing, type Files } from '@digi/regression/runtime/card-store.ts';
import { pokeKnob } from '@digi/regression/runtime/guest-mailbox.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { systemPackageFiles } from '@digi/regression/runtime/system-package.ts';
import { deviceListing } from '@digi/regression/runtime/upload-kit.ts';
import { PATH, REPLACE, SIZE, WRITE, cases, cutRig, oldModule, pattern, runJob } from './file-write-cut-support.ts';

// An SDK replace of a file cut at four sampled card writes: the old file or the new one at the path, never neither (setup and the host sweep: file-write-cut-support.ts).

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} SDK file replace: power cuts`, () => {
    const rig = cutRig(device);

    test('a replace cut or torn leaves the old file or the new one at the path, read whole by the next boot and by plusdrive', async () => {
      const { old, oldFile } = oldModule(rig, 'old-r.dkm');
      const prev = pattern(1500);
      const prevFile = path.join(rig.artifacts, 'prev.bin');
      fs.writeFileSync(prevFile, prev);
      const mod = rig.mod('test/file-write');
      const moduleBytes = fs.readFileSync(mod.file);
      const base = await rig.card({
        name: 'cut-replace-base',
        modules: [mod, { name: 'modules/old.dkm', file: oldFile }, { name: PATH, file: prevFile }],
        selection: [mod.name],
      });
      const oldFiles: Files = { ...systemPackageFiles(), 'modules/old.dkm': old, [mod.name]: moduleBytes, [PATH]: prev };
      const data = pattern(SIZE);
      for (const [k, [label, knobs, committed]] of cases.entries()) {
        const dk = await rig.pausedBoot(base);
        for (const [name, value] of Object.entries(knobs)) await pokeKnob(rig.session, rig.plan, name, value);
        const done = await runJob(rig, dk, SIZE, PATH, WRITE | REPLACE);
        expect(done.job === 0, `${label}: the replace is refused a write (status ${done.job})`).toBe(false);
        const card = await rig.after(`sdkrcut${k}`);

        const want: Files = committed ? { ...oldFiles, [PATH]: data } : oldFiles;
        const next = await rig.pausedBoot(card);
        expect(await deviceListing(next), `${label}: the next boot reads the old file or the new one, whole`).toEqual(listing(want));
        expect(await next.verify(PATH), `${label}: VERIFY of the file at the path`).toBe(sha(committed ? data : prev));
        if (committed) {
          const view = await rig.view(`sdkrcut${k}-v`);
          expect(listing(view.files), `${label}: plusdrive reads the same from the card ${view.head}`).toEqual(listing(want));
        }
      }
    }, 3_600_000);
  });
}
