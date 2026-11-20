// The rig's `setup` of a session: SETUP of the debug module (dk-session `trackSetup`), awaited on SETUP_DONE. Shared by `wb profile calibrate run`
// (tools/devtools/wb_calibrate.mjs) and the Profile pane (profile/calibration-run.ts). Host tests: tests/calibration-run.test.ts.
import {
  EventTimeout,
  RuntimeEvents,
  trackSetup,
  type Clock,
  type DkSession,
  type RuntimeEvent,
} from '@digi/dk-session';
import type { Rig } from './runner.ts';

/** Clock ms a SETUP request may wait for the debug module's track watch to have its baseline (about 2 s of emulated time after a boot). */
export const SETUP_READY_MS = 30_000;

/**
 * A machine given as a module id is the first machine type the runtime reports for that module (the MODULES reply, read when the step plays: the types change
 * with every load; a module with several machines calibrates its first one). A number is the type itself: the runner puts the stock machine (0, valid in every boot)
 * on a track to start and to end a case, and never restores a type the track had before the run (a run-time type may not be loaded in this boot). `track` is 1 to 16.
 * Effects are not set by SETUP.
 */
export function sessionSetup(
  session: Pick<DkSession, 'setup' | 'modules'>,
  events: Pick<RuntimeEvents, 'until' | 'tryUntil'>,
  clock: Clock,
): NonNullable<Rig['setup']> {
  return async (what, track, type) => {
    if (what === 'effect') throw new Error('SETUP sets machine and filter types, not effects');
    const t = trackSetup(session, events, { ready: { clock, withinMs: SETUP_READY_MS } });
    if (what === 'filter') {
      if (typeof type !== 'number') throw new Error('a filter type is a number');
      await t.setFilter(track - 1, type);
      return;
    }
    let machine: number;
    if (typeof type === 'number') machine = type;
    else {
      const m = (await session.modules()).find((x) => x.name === type);
      if (!m || m.types.length === 0) throw new Error(`module ${type} is not loaded or has no machine type`);
      machine = m.types[0]!;
    }
    await t.setMachine(track - 1, machine);
  };
}

/** `STABLE_BLOCKS` of the runtime when the boot record names none: 30 s of DSP blocks (1500 a second). A host test keeps it equal to `modules/core/runtime/abi.toml`. */
export const DEFAULT_STABLE_BLOCKS = 45_000;
const BLOCKS_PER_S = 1500;

/**
 * Wait until the runtime has called the running boot stable: its STABLE event, or an event whose block count is at least the boot's `stable_blocks` (BOOT_START, or
 * the default when the ring dropped it). The log's time is the firmware's block counter, which counts from the boot, so one recent event decides, whatever the ring
 * lost. The runtime counts boots that were not stable, and a second one in a row starts in safe mode with no module loaded, so a run that changes the loaded modules
 * by restarts must not restart earlier. The wait reads the log from the start of the boot with a cursor of its own. A device that was up for a long time logs nothing
 * new, so the wait first provokes an event with the block count now: the runtime logs every upload command that is not marked quiet (LIST). A younger boot waits for
 * its STABLE; when none comes in time (no log, or a boot that does not run) it throws, and the run stops instead of restarting into a boot that may be a safe-mode
 * one. `onWait` gets the seconds waited and the seconds of the stable period once a second.
 */
export async function untilBootStable(
  session: Pick<DkSession, 'logPage' | 'logConfig' | 'setLogConfig' | 'modules' | 'info'> &
    Partial<Pick<DkSession, 'list'>>,
  clock: Clock,
  onWait?: (waitedS: number, ofS: number) => void,
): Promise<void> {
  const events = new RuntimeEvents(session, clock);
  let stable = DEFAULT_STABLE_BLOCKS;
  try {
    await events.listen({ fromBoot: true });
    // the event of this command is stamped with the device's block count now (an idle boot has no newer one)
    await Promise.resolve(session.list?.()).catch(() => undefined);
    const match = (e: RuntimeEvent): boolean => {
      if (e.kind === 'BOOT_START') {
        if (Number(e.fields.stable_blocks) > 0) stable = Number(e.fields.stable_blocks);
      }
      return e.kind === 'STABLE' || e.blocks >= stable;
    };
    const start = await clock.now();
    for (;;) {
      try {
        await events.until('*', match, { withinMs: 1000 });
        return;
      } catch (e) {
        if (!(e instanceof EventTimeout)) throw e;
      }
      const waited = (await clock.now()) - start;
      if (waited >= (stable / BLOCKS_PER_S) * 1000 * 1.5 + 5000)
        throw new Error(`events.until *: not seen within ${waited} ms. Last events:\n${events.seen()}`);
      onWait?.(Math.round(waited / 1000), Math.round(stable / BLOCKS_PER_S));
    }
  } catch (e) {
    throw new Error(
      `the boot was not seen stable (${e instanceof Error ? e.message.split('\n')[0] : e}): the run stops here, because a restart now could start a safe-mode boot with no module loaded`,
    );
  }
}
