import fs from 'node:fs';
import { button } from '@digi/test-support';
import { decodeFault, readSettingsRecord, settingsValue } from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession, TestRuntime } from '@digi/regression/test-runtime.ts';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { DEBUG_MODULE } from '@digi/regression/runtime/debug-module.ts';
import { installWithDebug } from '@digi/regression/runtime/debug-install.ts';
import { moduleArtifact } from '@digi/regression/runtime/module-artifact.ts';
import { expectRuntimeReady } from '@digi/regression/runtime/ready.ts';
import { runtimeTests } from '@digi/regression/runtime/runtime-tests.ts';
import { readCrashSeen, RING_SYMBOLS } from '@digi/regression/runtime/runtime-menu.ts';
import { prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { pollEmulated, withEmulatedProbe } from '@digi/regression/runtime/waits.ts';
import { expectNoDebugModule, waitForSafeMode, waitForSafeUi } from './fault-trace-support.ts';

/** The fault tests that only the Digitakt II runs: the voice site of the DSP guard (the machine module of the test is a Digitakt II port) and the CPU crash record. */
const SITE_VOICE = 1;
const STAGE_CALL = 4;

/** A machine whose render never returns: the DT2 voice site. The machine is put on track 1 through the SRC list (the first run-time machine is the 8th row). */
async function expectVoiceHang(runtime: TestRuntime): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'fault-trace'), ['DKRT_BOOT']);
  await installWithDebug(runtime, 'test/dyn-dsp-hang');
  const { ui } = runtime;
  // input order is the firmware's: each key ends when the firmware took it
  await ui.tap('SRC', { held: 'FUNC', until: { drained: true } });
  for (let i = 0; i < 12; i++) await ui.tap('UP', { until: { drained: true } });
  for (let i = 0; i < 7; i++) await ui.tap('DOWN', { until: { drained: true } });
  await ui.tap('YES', { until: { drained: true } });
  await ui.tap('YES', { until: { drained: true } });
  await pollEmulated(
    session,
    async () =>
      (await withEmulatedProbe(session, (q) => dk.readLog(q))).some(
        (e) =>
          e.kind === LOG_KIND.MACHINE_CHANGE && e.args[0] === 0 && (e.args[2] ?? 0) >= 7 && e.args[2] !== 0xffffffff,
      ),
    Boolean,
    { emulatedMs: 180_000, intervalMs: 2000, message: 'the run-time machine is on track 1' },
  );
  await session.client.inputButton(button('TRIG 1'), true); // the render hangs on the third block of the note
  await waitForSafeMode(runtime);
  await waitForSafeUi(runtime, plan);

  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault).toMatchObject({ kind: 'dsp', site: SITE_VOICE, stage: STAGE_CALL, step: 0, module: 'dyn-dsp-hang' });
  expect(fault!.id, 'a run-time machine type').toBeGreaterThanOrEqual(7);
  await session.expectNoFault('voice hang');
}

/** A module that raises a CPU exception: the next boot keeps dk/fault with kind CPU crash and the PC. */
async function expectCpuCrashFault(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  // the daemon stops the run at a CPU exception unless the guest's handler (the runtime's crash capture) is to run
  await session.client.call('cpu.exceptions', { deliver: true });
  const cfg = Uint8Array.of(0, 0, 0x0b, 0xb8, 0); // 3000 blocks (2 s) after the first tick, an illegal instruction
  await dk.put('cfg/crash-after.bin', cfg);
  const device = session.options.device;
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'fault-trace'), ['DKRT_HOST_DIR', 'DKRT_BOOT', ...RING_SYMBOLS]);
  for (const id of [DEBUG_MODULE, 'test/dyn-crash']) {
    await dk.put(`modules/${id.split('/')[1]}.dkm`, fs.readFileSync(moduleArtifact(device, id)));
  }
  await dk.select(['modules/debug.dkm', 'modules/dyn-crash.dkm']);
  await dk.restart();
  // the module crashes this boot and the next one; the third boot is safe mode (two bad boots)
  await waitForSafeMode(runtime);
  await waitForSafeUi(runtime, plan);
  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault).toMatchObject({ kind: 'cpu', module: 'dyn-crash' });
  // the safe boot has no debug module: the runtime's own ring has the crash event, and the record's tail came from that ring
  const log = await expectNoDebugModule(runtime, plan);
  const crash = log.find((e) => e.kind === LOG_KIND.CRASH_FOUND);
  expect(fault!.at, 'the PC of the crash record').toBe(crash!.args[0]);
  expect((await readCrashSeen(session, plan)).tail, 'the last events before the crash').toBeGreaterThan(0);
}

runtimeTests('a machine that stops the DSP gives safe mode and names the module and the voice site', expectVoiceHang, { dsp: 'sharc',
  devices: ['dt2'],
});
// the crash comes 2 s after the first tick: the card keeps the runtime's 30 s stable period, so both crashed boots count as bad boots
runtimeTests('a CPU crash is kept as the last fault', expectCpuCrashFault, { devices: ['dt2'], stableBlocks: 0 });
