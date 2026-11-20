// The rig's `setup` of a session: SETUP of the debug module (dk-session `trackSetup`), awaited on SETUP_DONE. Shared by `wb profile calibrate run`
// (tools/devtools/wb_calibrate.mjs) and the Profile pane (profile/calibration-run.ts). Host tests: tests/calibration-run.test.ts.
import { RuntimeEvents, trackSetup, type Clock, type DkSession } from '@digi/dk-session';
import type { Rig } from './runner.ts';

/** Clock ms a SETUP request may wait for the debug module's track watch to have its baseline (about 2 s of emulated time after a boot). */
export const SETUP_READY_MS = 30_000;

/**
 * A machine given as a module id is the first machine type the runtime reports for that module (the MODULES reply, read when the step plays: the types change
 * with every load; a module with several machines calibrates its first one). `original` is the type the track had before this run's first machine SETUP on it. `track` is 1 to 16. Effects are not set by SETUP.
 */
export function sessionSetup(
  session: Pick<DkSession, 'setup' | 'modules'>,
  events: Pick<RuntimeEvents, 'until' | 'tryUntil'>,
  clock: Clock,
): NonNullable<Rig['setup']> {
  // the machine type each track had before the first machine SETUP on it: the old type of the firmware's own MACHINE_CHANGE (the debug module's watch of the types);
  // no change event means the track already had the requested type
  const original = new Map<number, number>();
  return async (what, track, type) => {
    if (what === 'effect') throw new Error('SETUP sets machine and filter types, not effects');
    const t = trackSetup(session, events, { ready: { clock, withinMs: SETUP_READY_MS } });
    if (what === 'filter') {
      if (typeof type !== 'number') throw new Error('a filter type is a number');
      await t.setFilter(track - 1, type);
      return;
    }
    let machine: number;
    if (type === 'original') {
      const was = original.get(track);
      if (was === undefined)
        throw new Error(`track ${track}: no machine type was set up on it by this run, so there is none to put back`);
      machine = was;
    } else if (typeof type === 'number') machine = type;
    else {
      const m = (await session.modules()).find((x) => x.name === type);
      if (!m || m.types.length === 0) throw new Error(`module ${type} is not loaded or has no machine type`);
      machine = m.types[0]!;
    }
    await t.setMachine(track - 1, machine);
    if (type !== 'original' && !original.has(track)) {
      const change = await events.tryUntil(
        'MACHINE_CHANGE',
        (e) => e.fields.track === track && e.fields.new_type === machine,
        { withinMs: 500 },
      );
      original.set(track, change && change.fields.old_type !== null ? Number(change.fields.old_type) : machine);
    }
  };
}

/** `STABLE_BLOCKS` of the runtime when the boot record names none: 30 s of DSP blocks (1500 a second). */
const DEFAULT_STABLE_BLOCKS = 45_000;
const BLOCKS_PER_S = 1500;

/**
 * Wait until the runtime has called the running boot stable: its STABLE event, or, when the ring dropped that event (counting floods it), an event whose block count is
 * the boot's `stable_blocks` (BOOT_START) past the boot's own. The runtime counts boots that were not stable, and a second one in a row starts in safe mode with no
 * module loaded, so a run that changes the loaded modules by restarts must not restart earlier. The bound is derived from the boot's `stable_blocks` with a margin;
 * when neither comes (no log, or a boot that does not run) it throws, and the run stops instead of restarting into a boot that may be a safe-mode one. The wait reads
 * the log from the start of the boot with a cursor of its own (the runner's waits have consumed earlier events).
 */
export async function untilBootStable(
  session: Pick<DkSession, 'logPage' | 'logConfig' | 'setLogConfig' | 'modules'>,
  clock: Clock,
): Promise<void> {
  const events = new RuntimeEvents(session, clock);
  let bootAt: number | undefined;
  let stable = DEFAULT_STABLE_BLOCKS;
  try {
    await events.listen({ fromBoot: true });
    await events.until(
      '*',
      (e) => {
        if (e.kind === 'BOOT_START') {
          bootAt = e.blocks;
          if (Number(e.fields.stable_blocks) > 0) stable = Number(e.fields.stable_blocks);
        }
        return e.kind === 'STABLE' || (bootAt !== undefined && e.blocks - bootAt >= stable);
      },
      { withinMs: (stable / BLOCKS_PER_S) * 1000 * 1.5 + 5000 },
    );
  } catch (e) {
    throw new Error(
      `the boot was not seen stable (${e instanceof Error ? e.message.split('\n')[0] : e}): the run stops here, because a restart now could start a safe-mode boot with no module loaded`,
    );
  }
}
