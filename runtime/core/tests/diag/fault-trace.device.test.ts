import { emulatorClock, waitClock } from '@digi/test-support';
import { decodeFault, readSettingsRecord, settingsValue } from '@digi/dk-session';
import { CMD, LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession, TestRuntime } from '@digi/regression/support.ts';
import { openSafeAbout } from '@digi/regression/runtime/flows/menu.ts';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { installWithDebug } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { readCrashSeen, RING_SYMBOLS } from '@digi/regression/runtime/runtime-menu.ts';
import { prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';
import { pollEmulated } from '@digi/regression/runtime/waits.ts';
import { expectNoDebugModule, waitForSafeMode, waitForSafeUi } from './fault-trace-support.ts';

const SOURCE_SHIFT = 8;
/** The watch declares a stop after 4500 blocks (3 s) without a heartbeat; the daemon emulates the DSP far slower than 1500 blocks a second. */
const BLOCKS_PAST_THE_LIMIT = 9000;

/** Wait until the DSP has run `blocks` more frames (1500 a second of emulated time, not wall time). */
async function waitFrames({ session }: PatchedSession, blocks: number): Promise<void> {
  await waitClock(emulatorClock(session), blocks / 1.5);
}
const SITE_FX_SEND = 3;
const STAGE_CALL = 4;
const SAFE_DSP = 4;
const SAFE_PANIC = 5;

/** An effect on the send route stops the DSP: the next boot is safe mode, and ABOUT, dk/fault and the log name the module, the site and the step. */
async function expectFxHang(runtime: TestRuntime): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'fault-trace'), [
    'DKRT_HOST_DIR',
    'DKRT_BOOT',
    ...RING_SYMBOLS,
  ]);
  await installWithDebug(runtime, 'test/dyn-fx-hang');
  await waitForSafeMode(runtime);
  await waitForSafeUi(runtime, plan);

  // the card keeps the fault
  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault, 'dk/fault is in the settings record').not.toBeNull();
  expect(fault).toMatchObject({
    kind: 'dsp',
    site: SITE_FX_SEND,
    stage: STAGE_CALL,
    step: 2, // the step of the last block that ended: the step 3 of the call that stopped the DSP is lost with the core
    module: 'dyn-fx-hang',
  });

  // safe mode loads no module, so the debug module is not there to answer LOG: the log of the safe boot is the runtime's own ring, which has the
  // finding and the reason, and the record's tail (taken from that ring at the stop) was found
  const log = await expectNoDebugModule(runtime, plan);
  const found = log.filter((e) => e.kind === LOG_KIND.FAULT_FOUND);
  expect(
    found.map((e) => e.args[0]! & 0xff),
    'FAULT_FOUND of kind DSP stopped',
  ).toEqual([2]);
  expect(found[0]!.args[2], 'the module step in the event').toBe(2);
  expect(log.filter((e) => e.kind === LOG_KIND.SAFE).map((e) => e.args[0])).toEqual([SAFE_DSP]);
  expect((await readCrashSeen(session, plan)).tail, 'the last events of the boot before').toBeGreaterThan(0);
  expect(
    (await dk.modules()).map((m) => m.name),
    'safe mode loads no module',
  ).toEqual([]);

  const rows = await openSafeAbout(runtime, plan);
  expect(rows).toEqual(
    expect.arrayContaining(['SAFE: DSP STOPPED', 'FAULT: DSP STOP', 'IN DYN-FX-HANG', 'FX SEND STEP 2']),
  );
  for (const row of rows) expect(row.length, row).toBeLessThanOrEqual(22);
  await session.expectNoFault('fx hang');
}

/** A send effect that faults in every block: FX_FAULT and FX_DISABLED, tagged with the module's source, and no stop. */
async function expectFxFaultEvents(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  await installWithDebug(runtime, 'test/dyn-fx-nan');
  await pollEmulated(session, async () => (await dk.readLog(true)).some((e) => e.kind === LOG_KIND.FX_DISABLED), Boolean, {
    emulatedMs: 180_000,
    intervalMs: 2000,
    message: 'the guard switched the row off',
  });
  const log = await dk.readLog();
  const faults = log.filter((e) => e.kind === LOG_KIND.FX_FAULT);
  const disabled = log.filter((e) => e.kind === LOG_KIND.FX_DISABLED);
  expect(faults.length).toBeGreaterThanOrEqual(1);
  expect(disabled).toHaveLength(1);
  for (const e of [...faults, ...disabled]) {
    expect(e.flags & 1, 'an error event').toBe(1);
    expect((e.flags >> SOURCE_SHIFT) & 0xff, 'the source is the module that owns the row').toBeGreaterThan(0);
    expect(e.args[0], 'the first free effect row').toBe(0);
  }
  expect(faults[faults.length - 1]!.args[1], 'faults counted').toBeGreaterThanOrEqual(8);
  const names = (await dk.modules()).map((m) => m.name);
  expect(names).toContain('test/dyn-fx-nan');
  expect((await dk.info()).safe_mode, 'a faulting effect is no stop').toBe(false);
  expect(log.some((e) => e.kind === LOG_KIND.DSP_STOPPED && (e.flags & 1) !== 0)).toBe(false);
  await session.expectNoFault('fx fault events');
}

/** An ordinary effect runs for longer than the stall limit: no stop is declared and the device does not reset. */
async function expectNoFalseStop(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  await installWithDebug(runtime, 'test/dyn-fx-gain-send');
  const boot = (await dk.readLog()).find((e) => e.kind === LOG_KIND.BOOT_START);
  await waitFrames(runtime, BLOCKS_PAST_THE_LIMIT);
  const log = await dk.readLog();
  expect(log.find((e) => e.kind === LOG_KIND.BOOT_START)?.seq, 'the same boot is still running').toBe(boot?.seq);
  expect(log.filter((e) => e.kind === LOG_KIND.DSP_STOPPED)).toEqual([]);
  expect((await dk.info()).safe_mode).toBe(false);
  expect((await dk.modules()).map((m) => m.name)).toContain('test/dyn-fx-gain-send');
  await session.expectNoFault('no false stop');
}

/** A panic of the runtime (CMD_FAULT, a failed bounds check) writes a panic record and resets: the next boot is safe mode and ABOUT, dk/fault and the log name it. */
async function expectRuntimePanic(runtime: TestRuntime): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'fault-trace'), [
    'DKRT_HOST_DIR',
    'DKRT_BOOT',
    ...RING_SYMBOLS,
  ]);
  // the device resets inside the handler and never replies; one try (quick), since a retry would reach the safe boot and panic it too
  await dk.request(CMD.FAULT, Uint8Array.of(1), true).catch(() => undefined);
  await waitForSafeMode(runtime);
  await waitForSafeUi(runtime, plan);

  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault, 'dk/fault is in the settings record').not.toBeNull();
  expect(fault).toMatchObject({ kind: 'panic', module: 'runtime', at: 0 });

  const log = await expectNoDebugModule(runtime, plan);
  const found = log.filter((e) => e.kind === LOG_KIND.FAULT_FOUND);
  expect(
    found.map((e) => e.args[0]! & 0xff),
    'FAULT_FOUND of kind panic',
  ).toEqual([3]);
  expect(log.filter((e) => e.kind === LOG_KIND.SAFE).map((e) => e.args[0])).toEqual([SAFE_PANIC]);
  expect((await readCrashSeen(session, plan)).panic, 'the boot found a panic record').toBe(1);

  const rows = await openSafeAbout(runtime, plan);
  expect(rows).toEqual(expect.arrayContaining(['SAFE: CPU PANIC', 'FAULT: CPU PANIC', 'IN RUNTIME']));
  for (const row of rows) expect(row.length, row).toBeLessThanOrEqual(22);
  await session.expectNoFault('runtime panic');
}

runtimeTests('an effect that stops the DSP gives safe mode and names the module, site and step', expectFxHang, { dsp: 'sharc',
});
runtimeTests('effect faults are logged with the module as source', expectFxFaultEvents, { dsp: 'sharc' });
runtimeTests('a normal effect is no stop', expectNoFalseStop, { dsp: 'sharc' });
runtimeTests('a panic of the runtime gives safe mode and names the runtime', expectRuntimePanic, {
  dsp: 'sharc',
});
