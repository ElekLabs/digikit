import {
  DEFAULT_HANG_MS,
  describeEmulator,
  emulatedClock,
  steppedClock,
  poll,
  waitClock as waitOnClock,
  type Clock,
  type EmuStatus,
  type PollOptions,
} from '@digi/dk-session';
import { PROGRESS_HANG_S, type Session } from './session.ts';

export type { Clock };

/** Wall ms a clock may stand still while a wait pauses it (a halted guest, a daemon that does not step) before the wait fails. `DIGI_CLOCK_STUCK_MS` overrides it. */
export const stuckMs = (): number => Number(process.env.DIGI_CLOCK_STUCK_MS ?? DEFAULT_HANG_MS);

const readStatus = (session: Pick<Session, 'status'>) => () => session.status() as unknown as Promise<EmuStatus>;

/** Free run (a running daemon): dk-session's `emulatedClock` over `session.status()`. */
export function freeClock(session: Pick<Session, 'status'>): Clock {
  return emulatedClock(readStatus(session));
}

/** `poll` with the stuck limit of `DIGI_CLOCK_STUCK_MS` (dk-session's `poll` is the one implementation). */
export function pollUntil<T>(
  clock: Clock,
  read: () => Promise<T>,
  done: (v: T) => boolean,
  opts: PollOptions,
): Promise<T> {
  return poll(clock, read, done, { ...opts, hangMs: opts.hangMs ?? stuckMs() });
}

/** `waitClock` of dk-session with the stuck limit of `DIGI_CLOCK_STUCK_MS`. */
export function waitClock(clock: Clock, ms: number, { hangMs = stuckMs() }: { hangMs?: number } = {}): Promise<void> {
  return waitOnClock(clock, ms, { hangMs });
}

/**
 * The emulator clock for both run modes: each `pause` reads `status.mode` and steps a paused daemon (dk-session's `steppedClock`) or waits on a
 * running one (`freeClock`). A fixture may switch modes inside a case (free while the device boots, paused for protocol reads).
 */
export function emulatorClock(session: Pick<Session, 'status' | 'client'>): Clock {
  const free = freeClock(session);
  const stepped = steppedClock(readStatus(session), (instructions, limit) => session.client.step(instructions, limit), {
    hangMs: PROGRESS_HANG_S * 1000,
  });
  return {
    now: () => free.now(),
    pause: async (ms) => {
      const status = (await session.status()) as { mode?: string };
      return status.mode === 'paused' ? stepped.pause(ms) : free.pause(ms);
    },
    describe: async () => describeEmulator(await session.status()),
    progress: () => free.progress?.(),
    pollMs: free.pollMs,
  };
}
