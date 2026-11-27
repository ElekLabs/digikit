// One wait for every host: a Clock (a source of time and a pause), `poll` on it, `waitClock` and the stuck guard.
// The wall clock serves hardware and the workbench; emu-clock.ts has the emulated one. No test, app or node-only imports.

/** Clock ms between rounds of a wait on an emulated clock: emulated time costs nothing extra. */
export const EMULATED_POLL_MS = 100;
/** Clock ms between rounds of a wait on the wall clock (hardware): every round is a request over the link. */
export const WALL_POLL_MS = 500;

/** Time for waits, in ms. `pause` lets about `ms` pass and may throw when time cannot pass. */
export interface Clock {
  now(): Promise<number>;
  pause(ms: number): Promise<void>;
  /** Waits until `ms` of emulated time have passed (a clock that has none lacks it); `hangMs`: the wall ms the clock may stand still before it throws. */
  pauseEmulated?(ms: number, hangMs?: number): Promise<void>;
  /** What the source reports (mode, CPU state, ...): added to the message of a stuck clock. */
  describe?(): Promise<string>;
  /** Movement other than `now` (a cold boot moves counters while `now` stands still), as a token of the last reading; a changed token counts as movement. */
  progress?(): string | undefined;
  /** Clock ms between rounds of a wait on this clock when the caller gives none (default WALL_POLL_MS). */
  pollMs?: number;
}

export const wallClock: Clock = {
  now: async () => Date.now(),
  pause: (ms) => new Promise((r) => setTimeout(r, ms)),
  pollMs: WALL_POLL_MS,
};

/** What one wait reports; the test harness turns it into a timing record. */
export interface WaitRecord {
  kind: 'event' | 'state' | 'time';
  what: string;
  clockMs: number;
  wallMs: number;
  ok: boolean;
}

/** Wall ms a clock may stand still before a wait fails (a halted or paused guest). */
export const DEFAULT_HANG_MS = 60_000;

/** Default failure bound of a boot or restart wait (ms of session time). A reboot with the DSP coupled and modules loading takes about 60-90 s; the bound leaves room for a slower host. A wait still ends as soon as the device is ready. */
export const BOOT_WAIT_MS = 180_000;

/** Throws when neither `clock.now` nor `clock.progress` moved for `hangMs` of wall time: a wait on a clock that never moves would end only at the test timeout. One guard per wait. */
export function stuckGuard(what: string, hangMs: number, clock: Clock): (now: number) => Promise<void> {
  let last: number | undefined;
  let lastProgress: string | undefined;
  let lastAdvance = Date.now();
  return async (now) => {
    const progress = clock.progress?.();
    const moved = last === undefined || now !== last || (lastProgress !== undefined && progress !== lastProgress);
    last = now;
    lastProgress = progress;
    if (moved) {
      lastAdvance = Date.now();
      return;
    }
    if (Date.now() - lastAdvance >= hangMs) {
      const detail = await clock.describe?.().catch((e: unknown) => `status failed: ${String(e)}`);
      throw new Error(
        `${what}: emulated clock stuck at ${now} ms for ${hangMs} ms (a halted or stopped guest?)${detail ? ` ${detail}` : ''}`,
      );
    }
  };
}

/** The deadline of a `poll` passed: the wait's own error, so a caller tells it from a failed read without matching text. */
export class PollTimeout extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PollTimeout';
  }
}

export interface PollOptions {
  withinMs: number;
  message: string | (() => string);
  intervalMs?: number;
  /** A throwing read counts as a miss; a function decides per error (false rethrows). */
  retry?: boolean | ((error: unknown) => boolean);
  onWait?: (r: WaitRecord) => void;
  /** Wall time the clock may stand still (default DEFAULT_HANG_MS). */
  hangMs?: number;
}

/**
 * Poll `read` until `done(value)`; fail after `withinMs` of clock time. The first read happens at once; the clock is read only after a miss.
 * `message` may be a function, resolved only on failure. Time counts only when the clock moves forward: a clock that drops (reboot, restore)
 * adds nothing and does not restart the budget.
 */
export async function poll<T>(
  clock: Clock,
  read: () => Promise<T>,
  done: (v: T) => boolean,
  opts: PollOptions,
): Promise<T> {
  const { withinMs, message, intervalMs = 200, retry = false, onWait, hangMs = DEFAULT_HANG_MS } = opts;
  const what = typeof message === 'string' ? message : 'poll';
  const stuck = stuckGuard(what, hangMs, clock);
  const wallStart = Date.now();
  let lastMs: number | undefined;
  let elapsed = 0;
  let clockMs = 0;
  let value: T | undefined;
  let failure: unknown;
  for (;;) {
    failure = undefined;
    try {
      value = await read();
      if (done(value)) {
        if (lastMs !== undefined)
          onWait?.({ kind: 'state', what, clockMs: elapsed, wallMs: Date.now() - wallStart, ok: true });
        return value;
      }
    } catch (error) {
      if (!(typeof retry === 'function' ? retry(error) : retry)) throw error;
      failure = error;
    }
    clockMs = await clock.now();
    await stuck(clockMs);
    if (lastMs !== undefined) elapsed += Math.max(0, clockMs - lastMs);
    lastMs = clockMs;
    onWait?.({ kind: 'state', what, clockMs: elapsed, wallMs: Date.now() - wallStart, ok: false });
    if (elapsed >= withinMs) {
      throw new PollTimeout(
        `${typeof message === 'function' ? message() : message}: not seen within ${withinMs} ms of emulated time (last: ${failure ? String(failure) : JSON.stringify(value)})`,
      );
    }
    await clock.pause(intervalMs);
  }
}

/** Let `ms` of clock time pass (an explicit fixed wait): pause in steps of at most 50 ms until now - start >= ms. Fails when the clock stands still for `hangMs` of wall time. */
export async function waitClock(
  clock: Clock,
  ms: number,
  { hangMs = DEFAULT_HANG_MS }: { hangMs?: number } = {},
): Promise<void> {
  const start = await clock.now();
  const stuck = stuckGuard(`wait of ${ms} ms`, hangMs, clock);
  for (let now = start; now - start < ms;) {
    await clock.pause(Math.min(50, ms - (now - start)));
    now = await clock.now();
    await stuck(now);
  }
}
