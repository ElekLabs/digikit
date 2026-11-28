// The emulator's clock and probe, from a `status()` function (the daemon's `status` reply), so dk-session needs no control-client.
// Emulated ms come from the ColdFire instruction count (`elapsed`, 132 M a second, counted from the last reset or restore in every mode); a
// reply without `elapsed` falls back to the DSP frame counter (1500 frames a second). The ColdFire runs whenever the daemon runs, so the clock
// still rises after a deliberate DSP stop and a wait ends by emulated time instead of hanging.
import type { EmulatedProbe } from './dkupload-core.ts';
import { DEFAULT_HANG_MS, EMULATED_POLL_MS, type Clock } from './waits.ts';

/** ColdFire instructions per emulated ms (132 M a second). */
export const INSNS_PER_MS = 132_000;
/** DSP frames per emulated ms (1500 a second): the fallback of a status reply without `elapsed`. */
export const FRAMES_PER_MS = 1.5;
/** Emulated ms one probe may take. A probe lost while the device restarts ends after this much emulated time (a slow host slows the clock too). */
export const PROBE_EMULATED_MS = 2000;
/** Wall ms after which a probe fails whatever the clock says: a guard for a dead link, never the limit of a healthy wait. */
const PROBE_WALL_GUARD_MS = 180_000;
/** Wall ms between clock readings while a probe waits. */
const PROBE_READ_MS = 100;
/** Wall ms between status readings while `pauseEmulated` waits for emulated time (a reading only observes progress). */
const PAUSE_READ_MS = 25;
/** Wall ms a `pauseEmulated` of `ms` emulated ms may see no movement of the emulated clock (and no job progress): `PAUSE_STUCK_BASE_MS` plus `PAUSE_STUCK_PER_MS` a ms. */
const PAUSE_STUCK_BASE_MS = 60_000;
const PAUSE_STUCK_PER_MS = 20;

/** The status fields that show a restart or load making progress (the daemon's `status` reply has more). */
export interface ProgressStatus {
  job?: {
    kind?: 'boot' | 'card' | 'step' | 'run_until' | 'firmware.load';
    wall_seconds?: number;
    phase?: string;
    progress?: number;
    icount?: number;
  } | null;
  epoch?: unknown;
  cf?: object | null;
}

export type EmuStatus = ProgressStatus & { audio?: unknown; elapsed?: unknown; mode?: string };

/** A job (cold boot, card load, stepping or `firmware.load`) is running: the emulated clock may stand still while it does. */
export function isLoading(status: ProgressStatus | undefined): boolean {
  return Boolean(status?.job);
}

function icount(counts: object | null | undefined): unknown {
  return (counts as { icount?: unknown } | null | undefined)?.icount;
}

/**
 * Movement other than the emulated clock, as a string that changes with it: the epoch and, only while a job runs, the ColdFire and job
 * instruction counts (a cold boot moves only these). Outside a job only the epoch counts, so a halted DSP with a running ColdFire is still stuck.
 */
export function restartProgress(status: ProgressStatus | undefined): string {
  const loading = isLoading(status);
  return [status?.epoch, loading ? icount(status?.cf) : undefined, loading ? icount(status?.job) : undefined].join('|');
}

/** The counts for a stuck message, empty when no job runs. */
export function describeRestart(status: ProgressStatus | undefined): string {
  if (!isLoading(status)) return '';
  return ` (job active: icount ${icount(status?.cf)}, job ${JSON.stringify(status?.job)})`;
}

/** Emulated milliseconds of a status reply. */
export function emulatedMsOf(status: { audio?: unknown; elapsed?: unknown }): number {
  if (status.elapsed !== undefined && status.elapsed !== null) return Number(status.elapsed) / INSNS_PER_MS;
  return Number((status.audio as { frames?: number } | null | undefined)?.frames ?? 0) / FRAMES_PER_MS;
}

/** What the emulator reports (mode, CPU state, phase, error, PC), for the message of a stuck clock. */
export function describeEmulator(status: object): string {
  const st = status as { mode?: string; epoch?: number; cf?: Record<string, unknown> };
  const cf = st.cf ?? {};
  const hex = (v: unknown) => (typeof v === 'number' ? `0x${v.toString(16)}` : String(v));
  return `[mode ${st.mode}, epoch ${st.epoch}, cpu_state ${cf.cpu_state}, phase ${cf.phase}, error ${JSON.stringify(cf.error)}, pc ${hex(cf.pc)}, icount ${cf.icount}, guard_hits ${cf.guard_hits}]`;
}

/**
 * Free run: `now` is the emulated ms of `status()`. `pause(ms)` waits `ms` of wall time through the global timer (vitest fake timers replace it) and reads nothing:
 * every wait calls `now()` after `pause`, and `now()` updates `progress()`. A clock that drops (reboot, restore) is handled by the waits: they count only forward movement.
 * No stuck guard in `pause`: `poll`, `waitClock` and the boot wait own one per wait (`stuckGuard`).
 *
 * `pauseEmulated(ms, hangMs?)` is the explicit emulated-time wait, for a capture window that must hold the same emulated time on any host speed: it returns when the emulated clock has advanced by `ms` (the same ms as `now`), so a
 * wait or a capture window holds the same emulated time on any host speed; it reads `status()` every few wall ms only to see the progress.
 * A clock that drops (reboot, restore) adds nothing: only forward movement counts. While a job (boot, load) holds the clock still, `pauseEmulated` returns after a wall interval, so the wait reads the state itself. When the clock stands still (and no job moves) for
 * `hangMs` of the caller or, without one, `PAUSE_STUCK_BASE_MS + PAUSE_STUCK_PER_MS * ms` of wall time (a paused or halted emulator), `pauseEmulated` throws with the emulator state.
 * The waits (`poll`, `waitClock`, the boot wait) keep their own stuck guards.
 */
export function emulatedClock(status: () => Promise<EmuStatus>): Clock {
  let progress: string | undefined;
  let lastReply: EmuStatus | undefined;
  const read = async () => {
    const reply = await status();
    lastReply = reply;
    progress = restartProgress(reply);
    return reply;
  };
  return {
    now: async () => emulatedMsOf(await read()),
    pause: async (ms) => {
      await new Promise<void>((r) => setTimeout(r, ms));
    },
    pauseEmulated: async (ms, hangMs) => {
      if (!(ms > 0)) return;
      let last = emulatedMsOf(await read());
      let waited = 0;
      let token = progress;
      let moved = Date.now();
      const stuckMs = hangMs ?? PAUSE_STUCK_BASE_MS + PAUSE_STUCK_PER_MS * ms;
      while (waited < ms) {
        await new Promise<void>((r) => setTimeout(r, PAUSE_READ_MS));
        const now = emulatedMsOf(await read());
        if (now > last) waited += now - last;
        if (now !== last || progress !== token) moved = Date.now();
        const advanced = now > last;
        last = now;
        token = progress;
        // a job (cold boot, load) may hold the emulated clock still: return after a wall interval so the wait reads its state
        if (!advanced && lastReply && isLoading(lastReply)) return;
        if (waited < ms && Date.now() - moved >= stuckMs) {
          const detail = await status().then(
            (r) => `${describeEmulator(r)}${describeRestart(r)}`,
            (e: unknown) => `status failed: ${String(e)}`,
          );
          throw new Error(
            `pauseEmulated(${ms} ms): emulated clock stuck at ${last.toFixed(1)} ms for ${stuckMs} ms of wall time after ${waited.toFixed(1)} ms (a paused or halted emulator?) ${detail}`,
          );
        }
      }
    },
    describe: async () => {
      const reply = await status();
      return `${describeEmulator(reply)}${describeRestart(reply)}`;
    },
    progress: () => progress,
    pollMs: EMULATED_POLL_MS,
  };
}

/** Emulated ms one `step` rpc of a stepped clock covers: a long window stays several short rpcs, none near the control client's timeout. */
const STEP_CHUNK_MS = 250;

/** What a `step` call is told: the wall seconds the daemon may spend on this one chunk. */
export interface StepLimit {
  timeout_wall_s: number;
}

/** Wall ms one step chunk may take before the clock fails (a wedged daemon), and the `timeout_wall_s` the daemon is given for it. */
export interface SteppedClockOptions {
  hangMs?: number;
}

async function stepEmulated(
  step: (instructions: number, limit: StepLimit) => Promise<unknown>,
  ms: number,
  hangMs: number,
): Promise<void> {
  for (let left = ms; left > 0; left -= STEP_CHUNK_MS) {
    const instructions = Math.ceil(Math.min(left, STEP_CHUNK_MS) * INSNS_PER_MS);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        step(instructions, { timeout_wall_s: hangMs / 1000 }),
        new Promise<never>((_resolve, reject) => {
          timer = setTimeout(
            () =>
              reject(
                new Error(`stepped clock: a step of ${instructions} instructions took over ${hangMs} ms of wall time`),
              ),
            hangMs,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  }
}

/**
 * A paused daemon: time passes only when stepped. `now` is the emulated ms of `status()`; `pause(ms)` steps the daemon by `ms` emulated ms
 * (`step(instructions, { timeout_wall_s })`, the daemon's `step`) instead of waiting wall time, so a wait bounded by N ms of emulated time ends when the
 * emulated clock has moved N ms, on any host speed and however many reads that takes. A chunk that takes over `hangMs` of wall time
 * (default `DEFAULT_HANG_MS`) fails the pause.
 */
export function steppedClock(
  status: () => Promise<EmuStatus>,
  step: (instructions: number, limit: StepLimit) => Promise<unknown>,
  { hangMs = DEFAULT_HANG_MS }: SteppedClockOptions = {},
): Clock {
  const base = emulatedClock(status);
  return {
    ...base,
    pause: (ms) => stepEmulated(step, ms, hangMs),
    pauseEmulated: (ms) => stepEmulated(step, ms, hangMs),
  };
}

/**
 * Bound one probe (`dk.info`, `dk.logPage`, ...) in emulated time: `run` gets `{ signal, ms }`; the signal fires `emulatedMs` of clock time after the
 * probe started (the clock only counts forward; a restart that lowers it starts the count again) and the probe then fails like a lost request.
 * Give the result to `DkSessionOptions.emulatedProbe`.
 */
export function emulatedProbe(clock: Pick<Clock, 'now'>, defaultMs = PROBE_EMULATED_MS): EmulatedProbe {
  return async <T>(
    run: (limit: { signal: AbortSignal; ms: number }) => Promise<T>,
    emulatedMs: number = defaultMs,
  ): Promise<T> => {
    const ctl = new AbortController();
    let live = true;
    const watch = (async () => {
      let last: number | undefined;
      let waited = 0;
      while (live) {
        await new Promise<void>((r) => setTimeout(r, PROBE_READ_MS));
        if (!live) break;
        const now = await clock.now();
        if (last !== undefined && now > last) waited += now - last;
        last = now;
        if (waited >= emulatedMs) return ctl.abort();
      }
    })().catch(() => {});
    try {
      return await run({ signal: ctl.signal, ms: PROBE_WALL_GUARD_MS });
    } finally {
      live = false;
      await watch;
    }
  };
}
