import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { RANGES } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { E_SET_NOKEY } from '@digi/regression/runtime/project-kit.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { apiContract, runtimeVersion } from '@digi/regression/runtime/upload-kit.ts';
import { pollEmulated, waitEmulated, waitForBootEmulated } from '@digi/regression/runtime/waits.ts';

/**
 * The entry point `dk_module` and the service imports (docs/plans/sdk.md section 5; the fixture test/entry-demo uses only the new contract).
 * `init` gets a `dk_module_ctx` and opens the settings; the optional USB bulk service is not in the image, so its `_open` answers DK_E_MISSING and the module
 * still loads; `tick` runs once per UI pass; a settings write queued by a host command runs in `tick`.
 */
const REPORT = 0x1c;
const WRITE = 0x1d;
const E_MISSING = -73;
const E_CONTRACT = -48;
const DEVICE_ID = { dt2: 1, dn2: 2 } as const; // [digikit] id of devices/*.toml

async function expectEntryPoint({ session, dk }: PatchedSession): Promise<void> {
  expect(['zero', 'none'], 'the entry point needs no DSP').toContain((await session.status()).dsp);
  await expectRuntimeReady(session);
  await installModule(dk, 'test/entry-demo', moduleArtifact(session.options.device, 'test/entry-demo'));
  expect(RANGES.entry_demo_test, 'reserved command range of the fixture').toEqual([0x1c, 0x1d]);
  const report = async () => {
    const reply = await dk.request(REPORT);
    expect(reply.status, 'the module registered its commands in init').toBe(0);
    const b = Buffer.from(reply.body);
    expect(b.length, 'report length').toBe(42);
    return {
      settingsOpen: b.readInt32BE(0),
      bootRead: b.readInt32BE(4),
      usbOpen: b.readInt32BE(8),
      usbHandleNull: b.readUInt32BE(12),
      device: b.readUInt32BE(16),
      runtime: b.readUInt32BE(20),
      ticks: b.readUInt32BE(24),
      tickSpan: b.readUInt32BE(28),
      put: b.readInt32BE(32),
      back: b.readInt32BE(36),
      value: b[40]!,
      pending: b[41]!,
    };
  };

  const first = await report();
  expect(first.settingsOpen, 'dk_settings_open in init').toBe(0);
  expect(first.bootRead, 'a fresh card has no "boot" value').toBe(E_SET_NOKEY);
  expect(first.usbOpen, 'dk_usb_bulk_open without core/usb-bulk is DK_E_MISSING').toBe(E_MISSING);
  expect(first.usbHandleNull, 'the absent service leaves the handle NULL').toBe(1);
  expect(first.device, 'dk_module_ctx_device').toBe(DEVICE_ID[session.options.device]);
  expect(first.runtime, 'dk_module_ctx_runtime_version').toBe(runtimeVersion());

  // The UI loop (and with it `tick`) starts some emulated seconds after the boot finished and the module registered: wait for the first tick (firmware
  // state), then let a second of emulated time pass.
  const running = await pollEmulated(session, report, (r) => r.ticks > 0, {
    emulatedMs: 30_000,
    message: 'the UI loop never called tick',
  });
  await waitEmulated(session, 1000);
  const later = await report();
  expect(later.ticks, 'tick runs on every UI pass').toBeGreaterThan(running.ticks);
  expect(later.tickSpan, 'dk_time_blocks advances between the first and the last tick').toBeGreaterThan(0);

  expect((await dk.request(WRITE, Uint8Array.of(0x5a))).status, 'the write is queued').toBe(0);
  await waitEmulated(session, 500);
  const written = await report();
  expect(written.pending, 'tick ran the queued write').toBe(0);
  expect([written.put, written.back, written.value], 'dk_settings_put, then dk_settings_get in tick').toEqual([0, 1, 0x5a]);
  await session.expectNoFault('entry point');
}

runtimeTests('a module of the new entry point loads, ticks and finds an optional service absent', expectEntryPoint, {
  dsp: 'zero',
});

/**
 * Version refusal. The packer writes the api contract (major 2) and the newest api minor its imports need (`since` in sdk/sdk.toml) into the header
 * (test/entry-demo: 2.11). A copy that declares an api minor above the runtime's (0xff), and one that declares major 1 (the Api table of the earlier runtimes),
 * each with the sha256 made right again so that only the version is wrong, are refused with E_CONTRACT and the module list shows no second entry; the file
 * with the real version loads after them.
 */
async function expectVersionRefusal({ session, dk }: PatchedSession): Promise<void> {
  const device = session.options.device;
  await expectRuntimeReady(session);
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'entry-version'), ['DKRT_BOOT']);
  const good = fs.readFileSync(moduleArtifact(device, 'test/entry-demo'));
  const [apiMajor, apiMinor] = apiContract();
  expect(good[0x38], 'the packer writes the api major of sdk/sdk.toml').toBe(apiMajor);
  expect(good[0x39], 'the newest `since` among the linked imports is the header minor').toBe(apiMinor);
  const withVersion = (major: number, minor: number): Buffer => {
    const file = Buffer.from(good);
    file[0x38] = major;
    file[0x39] = minor;
    file.fill(0, 0x10, 0x30);
    createHash('sha256').update(file).digest().copy(file, 0x10);
    return file;
  };
  await dk.put('modules/entry-old.dkm', withVersion(1, 11));
  await dk.put('modules/entry-future.dkm', withVersion(2, 0xff));
  await dk.put('modules/entry-demo.dkm', good);
  await dk.select(['modules/entry-old.dkm', 'modules/entry-future.dkm', 'modules/entry-demo.dkm']);
  await dk.restart();
  await waitForBootEmulated(dk);
  await expect
    .poll(async () => (await dk.modules()).map((m) => m.name), { timeout: 30_000, interval: 500 })
    .toEqual(['test/entry-demo']);
  const boot = await bootInfo(session, plan);
  expect(boot.results, 'the files of api major 1 and of a newer api minor are E_CONTRACT, the real one loads').toEqual([E_CONTRACT, E_CONTRACT, 0]);
  await session.expectNoFault('version refusal');
}

runtimeTests('a module that declares api major 1 or a newer api minor than the runtime is refused with E_CONTRACT', expectVersionRefusal, {
  dsp: 'zero',
});
