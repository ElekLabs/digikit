import { SetupRefused, trackSetup, type RuntimeEvents } from '@digi/dk-session';
import { SERVICE_CMD } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { TestRuntime } from '@digi/regression/support.ts';
import { installWithDebug } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';

/**
 * The debug module's SETUP command (calibration setup) on the firmware: a track's machine type and filter type are set through the firmware's own kit commit in the UI
 * task (the call a chooser makes), and SETUP_DONE reports the result.
 *
 * - Machine: type 1 on track 1 (shows the track's own type as the old type of the change), then 2, 1 and back to the own type. The firmware's own MACHINE_CHANGE (the
 *   debug module's watch of the machine types it reads back from the firmware) follows each change with the old and the new type. SETUP is refused until the watch
 *   has its baseline, so the test waits for it (the PARAM_CHANGE baseline follows it).
 * - Filter: type 2, 1, 2 on track 1. SETUP_DONE has status 0 only when the firmware's own filter getter reports the type afterwards.
 * - Refusals: a request for track 16 or an op that does not exist is refused at once; a filter type that is not a stock one is refused by the runtime in SETUP_DONE
 *   (-46) and changes nothing, as does a machine type in the gap between the stock types and the run-time range or in a run-time row nobody loaded.
 */
const TRACK = 0;
/** The machine watch scans every 20 ms: a few passes, in emulated time. */
const PASSES_MS = 200;
const WATCH_MS = 10_000;
const E_RANGE = -46;
/** Below the first run-time type and past the stock types of both devices (7 and 5): a built-in or gap row. */
const GAP_TYPE = 15;
/** The first run-time row: nothing is loaded in it here. */
const UNLOADED_TYPE = 16;
const E_PROTO = -60;

async function expectSetup(rt: TestRuntime): Promise<void> {
  const { dk, session } = rt;
  await expectRuntimeReady(session);
  await installWithDebug(rt);
  const events = await rt.listen({ fromBoot: true });
  // the core takes any run-time type of the device; that a module has loaded it is the host's check, from the MODULES reply
  const setup = trackSetup(dk, events, { runTime: { first: UNLOADED_TYPE, modules: () => dk.modules() } });
  await untilTrackBaseline(events);

  const machineChange = (type: number, withinMs: number) =>
    events.until('MACHINE_CHANGE', (e) => e.fields.track === TRACK + 1 && e.fields.new_type === type, { withinMs });
  // The wait on SETUP_DONE consumes the events before it, and the watch sees the change one pass (20 ms) later, so each change is awaited right after its own
  // SETUP_DONE. The kit's own type of track 1 is not known: asking for type 1 shows it as the old type of the change, and no change within a few passes means it was 1.
  await setup.setMachine(TRACK, 1);
  const first = await machineChange(1, PASSES_MS).catch(() => null);
  const original = first ? Number(first.fields.old_type) : 1;
  // from here every request changes the type, and the firmware's own machine watch must show it
  let current = 1;
  for (const target of [2, 1, original]) {
    if (target === current) continue;
    await setup.setMachine(TRACK, target);
    expect((await machineChange(target, WATCH_MS)).fields.old_type, `the change from type ${current} to ${target}`).toBe(current);
    current = target;
  }
  expect(current, 'the track is back at its own type').toBe(original);

  for (const type of [2, 1, 2]) expect(await setup.setFilter(TRACK, type), `filter type ${type}`).toMatchObject({ status: 0 });

  await expect(setup.setFilter(TRACK, 6), 'no filter row above the stock ones').rejects.toSatisfy(
    (e: unknown) => e instanceof SetupRefused && e.status === E_RANGE,
  );
  for (const type of [GAP_TYPE, UNLOADED_TYPE])
    await expect(setup.setMachine(TRACK, type), `machine type ${type} is no stock type and no loaded run-time type (15: the core refuses a gap row; 16: the host finds no module with it)`).rejects.toSatisfy(
      (e: unknown) => e instanceof SetupRefused && e.status === E_RANGE,
    );
  expect((await dk.request(SERVICE_CMD.SETUP, Uint8Array.of(1, 16, 1))).status, 'track 16').toBe(E_PROTO);
  expect((await dk.request(SERVICE_CMD.SETUP, Uint8Array.of(3, 0, 1))).status, 'an op that does not exist').toBe(E_PROTO);
  await session.expectNoFault('SETUP');
}

/** The debug module's track watch has taken its baseline: its first PARAM_CHANGE baseline (index 0xffffffff) follows the first scan. */
function untilTrackBaseline(events: RuntimeEvents) {
  return events.until('PARAM_CHANGE', (e) => e.args[1] === 0xffffffff, { withinMs: 30_000 });
}

runtimeTests('SETUP sets the machine and the filter type of a track through the firmware', expectSetup);
