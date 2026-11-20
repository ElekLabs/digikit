// The calibration setup of a track on a device with the debug module: its machine or filter type, set through the firmware's own kit commit in the UI task
// (SETUP, dkupload-core.ts `DkSession.setup`) and awaited on the log event SETUP_DONE, never on a fixed time.
import type { DkSession } from './dkupload-core.ts';
import type { RuntimeEvents } from './runtime-events.ts';

/** What the runner needs: each call returns once the firmware has performed the change, and throws `SetupRefused` when it did not take. */
export interface TrackSetup {
  setMachine(track: number, type: number): Promise<SetupDone>;
  setFilter(track: number, type: number): Promise<SetupDone>;
}

export interface SetupDone {
  seq: number;
  /** 0 */
  status: number;
}

/**
 * SETUP_DONE came with a status other than 0: `status` is the runtime's code. -46: a type no chooser offers. -53: the firmware kept the old type (the machine permit
 * refused it on that track, the project's settings mode makes the setter skip the write, or the read-back differs) or there is no project. -72: the debug module
 * was unloaded with the request queued. A request made before the module's track watch has its baseline is refused at once with -53 (a DkError, not this).
 */
export class SetupRefused extends Error {
  readonly status: number;
  constructor(what: string, track: number, type: number, status: number) {
    super(`setup ${what} track ${track} type ${type}: refused, status ${status}`);
    this.name = 'SetupRefused';
    this.status = status;
  }
}

/**
 * `events` must be listening (`events.listen()`) before the first call, so that no SETUP_DONE can pass between the request and the wait. One request
 * is in flight at a time: the debug module answers E_BUSY to a second one, and these calls wait for their own.
 */
export function trackSetup(
  dk: Pick<DkSession, 'setup'>,
  events: Pick<RuntimeEvents, 'until'>,
  { withinMs = 5000 }: { withinMs?: number } = {},
): TrackSetup {
  const run = async (what: 'machine' | 'filter', track: number, type: number): Promise<SetupDone> => {
    const seq = await dk.setup(what, track, type);
    const e = await events.until('SETUP_DONE', (x) => x.fields.seq === seq, { withinMs });
    const status = Number(e.fields.status);
    if (status !== 0) throw new SetupRefused(what, track, type, status);
    return { seq, status };
  };
  return {
    setMachine: (track, type) => run('machine', track, type),
    setFilter: (track, type) => run('filter', track, type),
  };
}
