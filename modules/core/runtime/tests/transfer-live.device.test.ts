import fs from 'node:fs';
import path from 'node:path';
import { fileHashHex } from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { portFile } from '@digi/regression/runtime/service-files.ts';
import { DEBUG_MODULE, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { cliInstall, transferCommand } from '@digi/regression/runtime/transfer-cli.ts';
import { expectPollEmulated } from '@digi/regression/runtime/waits.ts';

interface FolderResult {
  folder: string;
  continuous: boolean;
  files: { path: string; size: number; hash: string }[];
}

/** `wb transfer` upload (with services), persistence, folder mirroring and the selected-but-missing module boot. */
async function expectTransferLive({ session, dk }: PatchedSession): Promise<void> {
  const artifacts = liveArtifacts(session, 'transfer-live');
  const mirror = path.join(session.runDir, 'mirror');
  const device = session.options.device as 'dt2' | 'dn2';
  const modulePath = moduleStorePath(device, 'test/sine');
  // the debug module is installed next to it: the log of the last boot is read at the end
  const debugPath = moduleStorePath(device, DEBUG_MODULE);
  const mirrored = path.join(mirror, 'digikit', modulePath);
  const mirroredHash = async () => (fs.existsSync(mirrored) ? fileHashHex(fs.readFileSync(mirrored)) : null);

  await expectRuntimeReady(session);
  const epoch = (await session.status()).epoch;
  if (typeof epoch !== 'number') throw new Error('daemon status must report a numeric epoch');
  await cliInstall(session, 'test/sine', artifacts, 300_000, [DEBUG_MODULE]);
  expect((await session.status()).epoch, 'CLI RESTART boots another epoch').toBeGreaterThan(epoch);
  const entries = await dk.list();
  record(artifacts, 'installed-store', entries);
  const modulePaths = (files: { path: string }[]) => files.map((f) => f.path).filter((p) => !p.endsWith('.dks') && !p.endsWith('.dkport')).sort();
  expect(modulePaths(entries), 'upload survives the soft reset').toEqual([debugPath, modulePath].sort());
  // the services the modules need went on the card with them
  const services = entries.map((e) => e.path).filter((p) => p.startsWith('services/') && p.endsWith('.dks'));
  expect(services, 'the needed services are in the store listing').not.toEqual([]);
  expect(entries.map((e) => e.path).filter((p) => p.endsWith('.dkport')), 'with the port file of the firmware').toEqual([portFile(device).name]);
  const entry = entries.find((e) => e.path === modulePath)!;

  // The store follows the guest's card writes after its quiet period (about two seconds).
  await expectPollEmulated(session, mirroredHash, (value) => expect(value).toBe(entry.hash), { emulatedMs: 30000, intervalMs: 500, message: `mirror hash: ${mirrored}` });
  const folder = await session.client.call<FolderResult>('card.folder');
  record(artifacts, 'installed-folder', folder);
  expect(folder.folder, 'card.folder reports the session-owned mirror').toBe(mirror);
  expect(folder.continuous, 'card.folder continuously mirrors the store').toBe(true);
  expect(
    folder.files
      .filter((f) => !f.path.endsWith('.dks') && !f.path.endsWith('.dkport'))
      .map(({ path, hash }) => ({ path, hash }))
      .sort((x, y) => x.path.localeCompare(y.path)),
    'mirror metadata matches the card',
  ).toEqual(
    [
      { path: debugPath, hash: entries.find((e) => e.path === debugPath)!.hash },
      { path: modulePath, hash: entry.hash },
    ].sort((x, y) => x.path.localeCompare(y.path)),
  );

  const deleted = await transferCommand(session, ['delete', modulePath], path.join(artifacts, 'delete.log'), 300_000);
  expect(
    deleted.some((event) => event.event === 'delete'),
    'CLI reports deletion',
  ).toBe(true);
  const emptyStore = await dk.list();
  record(artifacts, 'deleted-store', emptyStore);
  expect(modulePaths(emptyStore), 'CLI delete removes the module and leaves the debug module').toEqual([debugPath]);
  const synced = await session.client.call<FolderResult>('card.sync');
  record(artifacts, 'deleted-folder', synced);
  expect(fs.existsSync(mirrored), 'card.sync removes the deleted module from the folder').toBe(false);
  expect(modulePaths(synced.files), 'card.sync reports the debug module only').toEqual([debugPath]);

  // DELETE keeps the selection. Exactly one more protocol restart preserves the original
  // boot-loop history: another early restart could enter safe mode instead of trying the path.
  await transferCommand(session, ['restart'], path.join(artifacts, 'restart.log'), 300_000);
  const events = await dk.readLog();
  record(artifacts, 'missing-module-boot', events);
  expect(
    events.some((event) => event.kind === LOG_KIND.MOD_LOAD && event.args[1] === 0xffffffff),
    'selected missing path logs MOD_LOAD E_NOT_FOUND; see missing-module-boot.json',
  ).toBe(true);
  expect(
    events.some((event) => event.kind === LOG_KIND.UI_FIRST),
    'boot reaches UI_FIRST after the missing module',
  ).toBe(true);
  const modules = await dk.modules();
  record(artifacts, 'missing-module-loaded', modules);
  expect(
    modules.map((module) => module.name).filter((name) => name === 'test/sine' || name === DEBUG_MODULE),
    'boot skips the missing selected module',
  ).toEqual([DEBUG_MODULE]);
}

runtimeTests('CLI transfer survives restart, mirrors the store and skips a deleted selection', expectTransferLive, { dsp: 'sharc',
  mirror: true,
});
