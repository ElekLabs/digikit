// The emulator's clock and probe, from a `status()` function (the daemon's `status` reply), so dk-session needs no control-client.
// Emulated ms come from the ColdFire instruction count (`elapsed`, 132 M a second, counted from the last reset or restore in every mode); a
// reply without `elapsed` falls back to the DSP frame counter (1500 frames a second). The ColdFire runs whenever the daemon runs, so the clock
// still rises after a deliberate DSP stop and a wait ends by emulated time instead of hanging.
import type { EmulatedProbe } from './dkupload-core.ts';
import { EMULATED_POLL_MS, type Clock } from './waits.ts';

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
 * Free run: `now` is the emulated ms of `status()`. `pause(ms)` waits `ms` of wall time through the global timer (vitest fake timers replace it),
 * and reads nothing: every wait calls `now()` after `pause`, and `now()` updates `progress()`. A clock that drops (reboot, restore) is handled by the waits: they count only forward movement.
 * No stuck guard here: `poll`, `waitClock` and the boot wait own one per wait (`stuckGuard`).
 */
export function emulatedClock(status: () => Promise<EmuStatus>): Clock {
  let progress: string | undefined;
  const read = async () => {
    const reply = await status();
    progress = restartProgress(reply);
    return reply;
  };
  return {
    now: async () => emulatedMsOf(await read()),
    pause: async (ms) => {
      await new Promise<void>((r) => setTimeout(r, ms));
    },
    describe: async () => {
      const reply = await status();
      return `${describeEmulator(reply)}${describeRestart(reply)}`;
    },
    progress: () => progress,
    pollMs: EMULATED_POLL_MS,
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
