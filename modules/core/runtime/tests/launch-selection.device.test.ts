import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileHashHex } from '@digi/dk-session';
import { describe, expect, test } from 'vitest';
import { daemonBinary, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { moduleArtifact } from '@digi/regression/runtime/install.ts';
import { expectRuntimeBoot, startPatchedSession, type PatchedSession } from '@digi/regression/support.ts';
import { install, libraryModule, restartDevice } from '../../../../apps/web/src/modules/dktransfer.ts';

/**
 * The Workbench's launch (`--device-folders` on a folder): the selection stays in the folder, so the next launch loads the modules at its
 * boot with no restart. Three daemons on one folder and one run directory, as three launches of the app:
 *  1. an empty folder: upload a module, SELECT, RESTART; the mirror writes the module and `digikit/selection.txt` to the folder;
 *  2. the card is built from that folder: the module is loaded at the boot, the boot epoch stays (no restart);
 *  3. nothing changed: the same card, so the cached ready state is used.
 * Without the selection file in the folder every launch has to select and restart again (the default-modules install, 15 to 20 s).
 */
const DEVICE = 'dn2';

describe(`DN2 ${FIRMWARE[DEVICE]}: the selection persists in the card folder`, () => {
  test(
    'a module selected in one launch is loaded at the boot of the next, and an unchanged launch reuses the boot cache',
    async () => {
      const root = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-launch-'));
      const folder = path.join(root, DEVICE);
      fs.mkdirSync(folder, { recursive: true });
      const runDir = path.join(root, 'run');
      fs.mkdirSync(runDir);
      const file = moduleArtifact(DEVICE, 'test/sine');
      const lib = (await libraryModule(fs.readFileSync(file)))!;
      const selection = () => path.join(folder, 'digikit/selection.txt');
      const launch = async (): Promise<{ rt: PatchedSession; seconds: number }> => {
        const began = Date.now();
        const rt = await startPatchedSession(
          {
            device: DEVICE,
            firmware: FIRMWARE[DEVICE],
            binary: daemonBinary(DEVICE),
            runDir,
            deviceFolders: true,
            env: { DIGI_PLUS_DRIVE: root },
          },
          'launch-selection',
        );
        try {
          await expectRuntimeBoot(rt);
        } catch (error) {
          await rt.close();
          throw error;
        }
        return { rt, seconds: (Date.now() - began) / 1000 };
      };
      const loaded = async (rt: PatchedSession) => (await rt.dk.modules()).map((m) => m.name);
      try {
        // launch 1
        const first = await launch();
        try {
          expect(await loaded(first.rt), 'an empty folder: nothing is selected').toEqual([]);
          const uploaded = await install(first.rt.dk, [lib.id], [lib]);
          expect(uploaded.uploaded).toEqual([lib.id]);
          await first.rt.dk.select([lib.path]);
          const epoch = (await first.rt.session.status()).epoch as number;
          expect((await restartDevice(first.rt.dk)).loaded).toEqual([lib.id]);
          expect((await first.rt.session.status()).epoch as number, 'the restart booted again').toBeGreaterThan(epoch);
          await first.rt.session.client.call('card.sync');
          expect(fs.readFileSync(selection(), 'utf8'), 'the folder holds the selection').toContain(lib.path);
          expect(fileHashHex(fs.readFileSync(path.join(folder, 'digikit', lib.path)))).toBe(
            fileHashHex(fs.readFileSync(file)),
          );
        } finally {
          await first.rt.close();
        }

        // launch 2: the module is loaded at the boot
        const second = await launch();
        try {
          const epoch = (await second.rt.session.status()).epoch;
          expect(await loaded(second.rt), 'the selection in the folder loads the module at the boot').toEqual([lib.id]);
          expect((await second.rt.session.status()).epoch, 'no restart was needed').toBe(epoch);
          console.log(`launch 2 (card built from the folder): ${second.seconds.toFixed(1)} s`);
        } finally {
          await second.rt.close();
        }

        // launch 3: nothing changed
        const third = await launch();
        try {
          expect(await loaded(third.rt)).toEqual([lib.id]);
          expect(third.rt.session.loadResult.cache_hit, 'the boot cache is used').toBe(true);
          console.log(`launch 3 (unchanged, boot cache hit): ${third.seconds.toFixed(1)} s`);
        } finally {
          await third.rt.close();
        }
      } finally {
        fs.rmSync(root, { recursive: true, force: true });
      }
    },
    900_000,
  );
});
