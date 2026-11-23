import { emulatorClock, moduleStorePathOf, waitClock, type Device } from '@digi/test-support';
import fs from 'node:fs';
import path from 'node:path';
import { fileHashHex, type DkLogEvent } from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { liveArtifacts, record } from '@digi/regression/runtime/artifacts.ts';
import { DEBUG_MODULE, installWithDebug, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady } from '@digi/regression/runtime/ready.ts';
import { neededServices, portFile, servicePath } from '@digi/regression/runtime/service-files.ts';
import { SYSTEM_PACKAGE_STORE_PATH, systemPackageFile } from '@digi/regression/runtime/system-package.ts';
import { pollEmulated } from '@digi/regression/runtime/waits.ts';
import type { RuntimeModule } from './manifests.ts';

/** What only the firmware and the card show of a module load: the module's bytes on the real card after the reboot, the DSP loader's completion on the SHARC, the machine's
 * voice row and type, guard counters, the firmware UI reached and no failure event in the log.
 * Owned below the device: the loader's version and runtime-needs fields and the MODULES reply (`assembly_tests.rs`,
 * `the_modules_reply_carries_the_version_and_the_needed_runtime_of_each_loaded_file`), the epoch change of a restart (`waitForRestart` in `installWithDebug` fails without one)
 * and the upload protocol (host tests of dkupload). The card's file set after the reboot stays here: it is a fact of the real card.
 * DSP allocation/registration does not establish rendered audio correctness. */
export async function expectModuleLoad(
  runtime: PatchedSession,
  module: RuntimeModule,
  artifact: string,
): Promise<void> {
  const { session, dk } = runtime;
  const bytes = fs.readFileSync(artifact);
  const storePath = moduleStorePathOf(module.id, bytes);
  // The debug module is loaded next to every module (the cell reads its LOG); for the debug module itself, once.
  const names = [...new Set([DEBUG_MODULE, module.id])];
  const device = session.options.device as Device;
  // the service files the module needs (services/<name>.dks) go on the card with it, and a service brings the port file of the device's firmware
  const services = neededServices([artifact]).map((s) => servicePath(s.service));
  // every test card holds the system package
  const paths = [
    ...new Set([SYSTEM_PACKAGE_STORE_PATH, moduleStorePath(device, DEBUG_MODULE), storePath, ...services, ...(services.length ? [portFile(device).name] : [])]),
  ].sort();
  const artifacts = path.join(liveArtifacts(session, 'module-load'), module.id.split('/')[1]!);
  fs.mkdirSync(artifacts, { recursive: true });

  await expectRuntimeReady(session);
  // the debug module is selected first: the log (LOG) exists only while it is loaded, and this cell reads it
  await installWithDebug(runtime, module.id);
  expect(
    (await dk.modules()).map((entry) => entry.name),
    'only the uploaded module and the debug module are registered',
  ).toEqual(names);
  const entries = await dk.list();
  record(artifacts, 'store', entries);
  expect(entries.map((entry) => entry.path).sort(), 'the card holds the system package, the module files and the service files they need after the reboot, no strays').toEqual(paths);
  const packageBytes = fs.readFileSync(systemPackageFile());
  expect(entries.find((entry) => entry.path === SYSTEM_PACKAGE_STORE_PATH), 'the system package is whole').toEqual({
    path: SYSTEM_PACKAGE_STORE_PATH,
    size: packageBytes.length,
    hash: fileHashHex(packageBytes),
  });
  expect(await dk.verifyFile(storePath, bytes), 'module bytes survive reboot').toBe(true);

  try {
    if (module.dsp) {
      // MOD_LOAD publishes the CF module before its DSP part completes over UI-loop passes.
      // This cell selects the debug module (index 0) and then this module's path, so boot's selection index is one (not MODULES' registry index).
      let completionLog: DkLogEvent[] = [];
      try {
        await pollEmulated(
          session,
          async () => {
            completionLog = await dk.readLog();
            return completionLog.some((event) => event.kind === LOG_KIND.MOD_DSP && event.args[0] === 1);
          },
          Boolean,
          { emulatedMs: 30_000, intervalMs: 500, message: 'selected module DSP completion' },
        );
      } finally {
        record(artifacts, 'dsp-completion-log', completionLog);
      }
      expect(
        completionLog
          .filter((event) => event.kind === LOG_KIND.MOD_DSP && event.args[0] === 1)
          .map((event) => event.args[1]),
        'DSP loader completes the selected module successfully',
      ).toEqual([0]);
    }
    const loaded = (await dk.modules()).find((entry) => entry.name === module.id)!;
    if (module.dsp) {
      expect(loaded.dsp_len, 'module DSP image was allocated').toBeGreaterThan(0);
      if (module.type === 'machine') {
        expect(loaded.rows, 'machine has a published DSP voice row').toBeGreaterThan(0);
        expect(loaded.types, 'machine has a published firmware type').toHaveLength(1);
      }
    } else {
      expect(loaded.dsp_len).toBe(0);
    }
    // DSP registrations need time to run on the coupled DSP before checking guard counters.
    // CF-only registration is already complete; UI and fault acceptance below still apply.
    if (module.dsp) {
      expect((await session.status()).dsp, 'DSP conformance remains coupled').toBe('sharc');
      await waitClock(emulatorClock(session), 2000); // DSP registrations run on the coupled DSP: no observable end
    } else {
      const status = await session.status();
      expect(status.dsp, 'CF-only conformance retains the zero SSI peer').toBe('zero');
      expect(status.audio, 'CF-only conformance has no SHARC audio session').toBeNull();
    }
    const modules = await dk.modules();
    record(artifacts, 'modules', modules);
    expect(modules.map(({ name, faults, guard_off }) => ({ name, faults, guard_off }))).toEqual(
      names.map((name) => ({ name, faults: 0, guard_off: false })),
    );
    await pollEmulated(
      session,
      async () => (await dk.readLog()).some((event) => event.kind === LOG_KIND.UI_FIRST),
      Boolean,
      { emulatedMs: 30_000, intervalMs: 500, message: 'the firmware UI is reached' },
    );
    const log = await dk.readLog();
    record(artifacts, 'boot-log', log);
    expect(
      log.some((event) => event.kind === LOG_KIND.UI_FIRST),
      'selected module boot reaches the firmware UI',
    ).toBe(true);
    const failures = new Set<number>([
      LOG_KIND.SAFE,
      LOG_KIND.DSP_STOPPED,
      LOG_KIND.CRASH_FOUND,
      LOG_KIND.DSP_ABSENT,
      LOG_KIND.MACHINE_FAULT,
      LOG_KIND.MACHINE_DISABLED,
      LOG_KIND.TABLE_BAD,
    ]);
    expect(
      log.filter((event) => failures.has(event.kind)),
      'no runtime crash, missing DSP or disabled registration',
    ).toEqual([]);
    await session.expectNoFault(`loaded ${module.id}`);
  } finally {
    record(artifacts, 'final-modules', await dk.modules());
    record(artifacts, 'final-status', await session.status());
  }
}
