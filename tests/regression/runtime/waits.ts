// Waits of the device tests: thin callers of dk-session's one `poll`, `emulatedClock` and `emulatedProbe`. No second implementation.
import {
  DkSession,
  EmuUsbTransport,
  emulatedProbe,
  emulatedMsOf,
  PROBE_EMULATED_MS,
  type DkSessionOptions,
} from '@digi/dk-session';
import {
  deadlineExpired,
  freeClock,
  pollUntil,
  withinRuntimeDeadline as beforeReadinessDeadline,
  type Session,
} from '@digi/test-support';

/** Emulated milliseconds of a status reply. */
export { emulatedMsOf as emulatedClockMs };

/** Emulated ms one probe of a device wait may take (see `emulatedProbe` in dk-session). */
export { PROBE_EMULATED_MS };

/** The clock and the probe of a DkSession over the emulator: the free emulated clock and probes bounded in emulated time. One constructor for the harness, `wb --emu` and the tools. */
export function emulatedOptions(session: Pick<Session, 'status'>): Pick<DkSessionOptions, 'clock' | 'emulatedProbe'> {
  const clock = freeClock(session);
  return { clock, emulatedProbe: emulatedProbe(clock) };
}

/** A DkSession over the emulator's USB with `emulatedOptions`. */
export function emulatorDk(
  session: Pick<Session, 'client' | 'status'>,
  options: Omit<DkSessionOptions, 'clock' | 'emulatedProbe'> = {},
): DkSession {
  return new DkSession(new EmuUsbTransport((method, params) => session.client.call(method, params)), {
    ...options,
    ...emulatedOptions(session),
  });
}

/** Run one probe bounded in emulated time (`run` gets a `Quick` whose signal fires after `emulatedMs`). */
export function withEmulatedProbe<T>(
  session: Pick<Session, 'status'>,
  run: (quick: { signal: AbortSignal; ms: number }) => Promise<T>,
  emulatedMs = PROBE_EMULATED_MS,
): Promise<T> {
  return emulatedProbe(freeClock(session))(run, emulatedMs);
}

/**
 * Poll `read` until `done(value)` (`retry` also retries a failing `read`); fail with `message` and the last value after `emulatedMs` of emulated time.
 * A stuck or dead daemon is not this helper's job beyond the clock guard (`DIGI_CLOCK_STUCK_MS`): the no-progress watchdog closes the session and `read` rejects.
 */
export function pollEmulated<T>(
  session: Pick<Session, 'status'>,
  read: () => Promise<T>,
  done: (value: T) => boolean,
  {
    emulatedMs,
    ...rest
  }: { emulatedMs: number; message: string | (() => string); intervalMs?: number; retry?: boolean; hangMs?: number },
): Promise<T> {
  return pollUntil(freeClock(session), read, done, { withinMs: emulatedMs, ...rest });
}

/**
 * `expect.poll` in emulated time: retry `read` (a failing read counts as a miss) until `assertion(value)` stops throwing, and fail with `message`
 * and the last assertion error after `emulatedMs` of emulated time.
 */
export function expectPollEmulated<T>(
  session: Pick<Session, 'status'>,
  read: () => Promise<T>,
  assertion: (value: T) => void,
  {
    emulatedMs = 30_000,
    message = 'poll',
    intervalMs = 200,
  }: { emulatedMs?: number; message?: string | (() => string); intervalMs?: number } = {},
): Promise<T> {
  return pollEmulated(
    session,
    async () => {
      const value = await read();
      assertion(value);
      return value;
    },
    () => true,
    { emulatedMs, message, intervalMs, retry: true },
  );
}

/** Shared lifecycle deadline; preserve the existing readiness-helper export for its callers. */
export { beforeReadinessDeadline, deadlineExpired };
