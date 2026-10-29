import fs from 'node:fs';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { button } from '@digi/test-support';
import { expect } from 'vitest';
import type { PatchedSession } from '../support.ts';
import { decodeFault, readSettingsRecord, settingsValue } from './fault.ts';
import { tap as holdTap, installModule, liveArtifacts, moduleArtifact, runtimeTests, settleUi } from './live.ts';
import { openDigikitMenu, readAboutRows } from './runtime-menu.ts';
import { prepareRuntimePlan } from './runtime-plan.ts';

const SOURCE_SHIFT = 8;
/** The watch declares a stop after 4500 blocks (3 s) without a heartbeat; the daemon emulates the DSP far slower than 1500 blocks a second. */
const BLOCKS_PAST_THE_LIMIT = 9000;
/** After a reset the UI needs about ten seconds before it takes keys (the input-adapter test waits the same). */
const SETTLE_BLOCKS = 15_000;

/** Wait until the DSP has run `blocks` more frames (emulated time, not wall time). */
async function waitFrames({ session }: PatchedSession, blocks: number): Promise<void> {
  const frames = async () => Number(((await session.status()).audio as { frames?: number } | null)?.frames ?? 0);
  const start = await frames();
  await expect
    .poll(async () => (await frames()) - start, { timeout: 900_000, interval: 1000, message: `${blocks} DSP blocks` })
    .toBeGreaterThanOrEqual(blocks);
}
const SITE_VOICE = 1;
const SITE_FX_SEND = 3;
const STAGE_CALL = 4;
const SAFE_DSP = 4;

/** Press, hold and release a key, then let the UI settle, all in DSP blocks (emulated time): the daemon may run far slower than real time, and a wall-clock
 * settle would find the UI not yet done. `settleMs` is the milliseconds the other runtime tests wait; a block is 2/3 ms, and a third of the time suffices here. */
function panelTap(runtime: PatchedSession) {
  const { session } = runtime;
  return async (name: string, settleMs = 1200) => {
    const code = button(name);
    await session.client.inputButton(code, true);
    await waitFrames(runtime, 150);
    await session.client.inputButton(code, false);
    await waitFrames(runtime, Math.round(settleMs / 2));
  };
}

/** The rows of the ABOUT page, opened through the panel like a user does. */
async function aboutRows(runtime: PatchedSession, plan: Awaited<ReturnType<typeof prepareRuntimePlan>>) {
  // wall-clock taps like the input-adapter test: on the Digitakt II the block-timed taps above did not open SETTINGS after the safe boot
  const tap = async (name: string, settleMs = 1200) => {
    const code = button(name);
    await holdTap((down) => runtime.session.client.inputButton(code, down), 300, settleMs);
  };
  await openDigikitMenu(tap);
  // in safe mode opening DIGIKIT shows a dialog that names the reason (NO closes it), and the first row is the SAFE notice: ABOUT is the second
  await tap('NO', 1000);
  // the cursor may rest below the first row after the dialog: go to the top, then to ABOUT
  await tap('UP', 600);
  await tap('UP', 600);
  await tap('UP', 600);
  await tap('DOWN');
  await tap('YES', 3000);
  return readAboutRows(runtime.session, plan);
}

/** Wait for the device to restart itself into safe mode (the hang record is written about 20 s after the DSP stopped). */
async function waitForSafeMode({ dk }: PatchedSession): Promise<void> {
  await expect
    .poll(async () => (await dk.info(true)).safe_mode, {
      timeout: 900_000,
      interval: 2000,
      message: 'safe mode after the stop',
    })
    .toBe(true);
}

/** An effect on the send route stops the DSP: the next boot is safe mode, and ABOUT, dk/fault and the log name the module, the site and the step. */
async function expectFxHang(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'fault-trace'), ['DK_ABOUT', 'DK_ABOUT_N']);
  await installModule(dk, 'test/dyn-fx-hang', moduleArtifact(session.options.device, 'test/dyn-fx-hang'));
  await waitForSafeMode(runtime);
  await waitFrames(runtime, SETTLE_BLOCKS);

  // the card keeps the fault
  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault, 'dk/fault is in the settings record').not.toBeNull();
  expect(fault).toMatchObject({
    kind: 'dsp',
    site: SITE_FX_SEND,
    stage: STAGE_CALL,
    step: 3,
    module: 'dyn-fx-hang',
  });

  // the log of the safe boot has the finding and the reason
  const log = await dk.readLog();
  const found = log.filter((e) => e.kind === LOG_KIND.FAULT_FOUND);
  expect(
    found.map((e) => e.args[0]! & 0xff),
    'FAULT_FOUND of kind DSP stopped',
  ).toEqual([2]);
  expect(found[0]!.args[2], 'the module step in the event').toBe(3);
  expect(log.filter((e) => e.kind === LOG_KIND.SAFE).map((e) => e.args[0])).toEqual([SAFE_DSP]);
  expect(
    log.some((e) => e.kind === LOG_KIND.CRASH_TAIL),
    'the last events of the boot before',
  ).toBe(true);
  expect(
    (await dk.modules()).map((m) => m.name),
    'safe mode loads no module',
  ).toEqual([]);

  const rows = await aboutRows(runtime, plan);
  expect(rows).toEqual(
    expect.arrayContaining(['SAFE: DSP STOPPED', 'FAULT: DSP STOP', 'IN DYN-FX-HANG', 'FX SEND STEP 3']),
  );
  for (const row of rows) expect(row.length, row).toBeLessThanOrEqual(22);
  await session.expectNoFault('fx hang');
}

/** A machine whose render never returns: the DT2 voice site. The machine is put on track 1 through the SRC list (the first run-time machine is the 8th row). */
async function expectVoiceHang(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  await installModule(dk, 'test/dyn-dsp-hang', moduleArtifact(session.options.device, 'test/dyn-dsp-hang'));
  const tap = panelTap(runtime);
  await session.client.inputButton(button('FUNC'), true);
  await tap('SRC', 1200);
  await session.client.inputButton(button('FUNC'), false);
  for (let i = 0; i < 12; i++) await tap('UP', 300);
  for (let i = 0; i < 7; i++) await tap('DOWN', 300);
  await tap('YES', 1200);
  await tap('YES', 1200);
  await expect
    .poll(
      async () =>
        (await dk.readLog(true)).some(
          (e) =>
            e.kind === LOG_KIND.MACHINE_CHANGE && e.args[0] === 0 && (e.args[2] ?? 0) >= 7 && e.args[2] !== 0xffffffff,
        ),
      { timeout: 300_000, interval: 2000, message: 'the run-time machine is on track 1' },
    )
    .toBe(true);
  await session.client.inputButton(button('TRIG 1'), true); // the render hangs on the third block of the note
  await waitForSafeMode(runtime);
  await waitFrames(runtime, SETTLE_BLOCKS);

  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault).toMatchObject({ kind: 'dsp', site: SITE_VOICE, stage: STAGE_CALL, step: 0, module: 'dyn-dsp-hang' });
  expect(fault!.id, 'a run-time machine type').toBeGreaterThanOrEqual(7);
  await session.expectNoFault('voice hang');
}

/** A send effect that faults in every block: FX_FAULT and FX_DISABLED, tagged with the module's source, and no stop. */
async function expectFxFaultEvents(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  await installModule(dk, 'test/dyn-fx-nan', moduleArtifact(session.options.device, 'test/dyn-fx-nan'));
  await expect
    .poll(async () => (await dk.readLog(true)).some((e) => e.kind === LOG_KIND.FX_DISABLED), {
      timeout: 600_000,
      interval: 2000,
      message: 'the guard switched the row off',
    })
    .toBe(true);
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
  await settleUi(session);
  await installModule(dk, 'test/dyn-fx-gain-send', moduleArtifact(session.options.device, 'test/dyn-fx-gain-send'));
  const boot = (await dk.readLog()).find((e) => e.kind === LOG_KIND.BOOT_START);
  await waitFrames(runtime, BLOCKS_PAST_THE_LIMIT);
  const log = await dk.readLog();
  expect(log.find((e) => e.kind === LOG_KIND.BOOT_START)?.seq, 'the same boot is still running').toBe(boot?.seq);
  expect(log.filter((e) => e.kind === LOG_KIND.DSP_STOPPED)).toEqual([]);
  expect((await dk.info()).safe_mode).toBe(false);
  expect((await dk.modules()).map((m) => m.name)).toContain('test/dyn-fx-gain-send');
  await session.expectNoFault('no false stop');
}

/** A module that raises a CPU exception: the next boot keeps dk/fault with kind CPU crash and the PC. */
async function expectCpuCrashFault(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await settleUi(session);
  // the daemon stops the run at a CPU exception unless the guest's handler (the runtime's crash capture) is to run
  await session.client.call('cpu.exceptions', { deliver: true });
  const cfg = Uint8Array.of(0, 0, 0x0b, 0xb8, 0); // 3000 blocks (2 s) after the first slot call, an illegal instruction
  await dk.put('cfg/crash-after.bin', cfg);
  const device = session.options.device;
  const bytes = fs.readFileSync(moduleArtifact(device, 'test/dyn-crash'));
  await dk.put('modules/dyn-crash.dkm', bytes);
  await dk.select(['modules/dyn-crash.dkm']);
  await dk.restart();
  // the module crashes this boot and the next one; the third boot is safe mode (two bad boots)
  await waitForSafeMode(runtime);
  await waitFrames(runtime, SETTLE_BLOCKS);
  const fault = decodeFault(settingsValue(await readSettingsRecord(dk), 'dk/fault') ?? new Uint8Array());
  expect(fault).toMatchObject({ kind: 'cpu', module: 'dyn-crash' });
  const log = await dk.readLog();
  const crash = log.find((e) => e.kind === LOG_KIND.CRASH_FOUND);
  expect(fault!.at, 'the PC of the crash record').toBe(crash!.args[0]);
}

runtimeTests('an effect that stops the DSP gives safe mode and names the module, site and step', expectFxHang, {
  timeout: 1_200_000,
});
runtimeTests('a machine that stops the DSP gives safe mode and names the module and the voice site', expectVoiceHang, {
  timeout: 1_200_000,
  devices: ['dt2'],
});
runtimeTests('effect faults are logged with the module as source', expectFxFaultEvents, { timeout: 900_000 });
runtimeTests('a normal effect is no stop', expectNoFalseStop, { timeout: 1_200_000 });
runtimeTests('a CPU crash is kept as the last fault', expectCpuCrashFault, { timeout: 1_200_000, devices: ['dt2'] });
