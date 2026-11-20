// The rig's `setup` of a session: SETUP of the debug module (dk-session `trackSetup`), awaited on SETUP_DONE. Shared by `wb profile calibrate run`
// (tools/devtools/wb_calibrate.mjs) and the Profile pane (profile/calibration-run.ts). Host tests: tests/calibration-run.test.ts.
import { RuntimeEvents, trackSetup, type Clock, type DkSession } from '@digi/dk-session';
import type { Rig } from './runner.ts';

/** Clock ms a SETUP request may wait for the debug module's track watch to have its baseline (about 2 s of emulated time after a boot). */
export const SETUP_READY_MS = 30_000;

/**
 * A machine given as a module id is the first machine type the runtime reports for that module (the MODULES reply, read when the step plays: the types change
 * with every load). `track` is 1 to 16. Effects are not set by SETUP.
 */
export function sessionSetup(
  session: Pick<DkSession, 'setup' | 'modules'>,
  events: Pick<RuntimeEvents, 'until'>,
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

/** Clock ms the runtime may need to call a boot stable (`STABLE_BLOCKS`, 30 s of DSP blocks) and a margin. */
export const STABLE_WAIT_MS = 45_000;

/**
 * Wait until the runtime has called the running boot stable (its STABLE event). The runtime counts boots that did not last that long, and a second one in a
 * row starts in safe mode with no module loaded: a run that changes the loaded modules by restarts must not restart earlier. The wait reads the log from the
 * start of the boot with a cursor of its own (the runner's waits have consumed earlier events). A boot whose STABLE event is not found (the ring dropped
 * it, or there is no log) is not waited for any longer: the restart goes on and the runtime decides.
 */
export async function untilBootStable(
  session: Pick<DkSession, 'logPage' | 'logConfig' | 'setLogConfig' | 'modules'>,
  clock: Clock,
  log: (line: string) => void = () => {},
): Promise<void> {
  const events = new RuntimeEvents(session, clock);
  try {
    await events.listen({ fromBoot: true });
    await events.until('STABLE', undefined, { withinMs: STABLE_WAIT_MS });
  } catch (e) {
    log(`the boot was not seen stable (${e instanceof Error ? e.message.split('\n')[0] : e}); restarting anyway`);
  }
}
