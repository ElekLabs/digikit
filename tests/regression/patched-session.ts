import { randomUUID } from 'node:crypto';
import { afterAll, afterEach, beforeAll } from 'vitest';
import { Session, type SessionOptions } from '@digi/test-support';
import { emulatorDk } from './runtime/waits.ts';
import type { TestRuntime } from './test-runtime.ts';
import { setTimingContext, timePhase } from './timing.ts';
import { startNoProgressWatchdog, type Watchdog } from './watchdog.ts';
import { withWaitApi } from './wait-api.ts';

/** A fresh runtime daemon; protocol cancellation precedes daemon shutdown. No remote module is assumed installed. */
export async function startPatchedSession(
  opts: Omit<SessionOptions, 'runtime' | 'dsp'> & { patchSet?: string; dsp?: SessionOptions['dsp'] },
  scope = 'patched runtime',
): Promise<TestRuntime> {
  const context = { scope, device: opts.device, session: randomUUID() };
  // The watchdog starts with the daemon, so it covers the load inside `Session.start`.
  let watchdog: Watchdog | undefined;
  const session = await timePhase(
    'session.startup',
    () =>
      Session.start({
        ...opts,
        runtime: { patchSet: opts.patchSet },
        onSession: (started) => {
          watchdog = startNoProgressWatchdog(started);
        },
      }).catch((error) => {
        watchdog?.stop();
        throw error;
      }),
    context,
  );
  setTimingContext(session, { ...context, dsp: session.dsp });
  try {
    const dk = emulatorDk(session);
    let closing: Promise<void> | undefined;
    return withWaitApi({
      session,
      dk,
      close() {
        watchdog?.stop();
        return (closing ??= timePhase(
          'session.shutdown',
          async () => {
            dk.cancelPending();
            await session.close();
          },
          context,
        ));
      },
    });
  } catch (err) {
    watchdog?.stop();
    session.keepDiagnostics();
    await timePhase('session.shutdown', () => session.close(), context);
    throw err;
  }
}

/** No restart hook: every acceptance file owns a single fresh, free-running runtime. Vitest forbids `expect.poll` in
 * `beforeAll`, so each test calls `expectRuntimeBoot` itself. */
export function withPatchedSession(
  opts: Omit<SessionOptions, 'runtime' | 'dsp'> & { patchSet?: string; dsp?: SessionOptions['dsp'] },
  scope?: string,
): {
  readonly runtime: TestRuntime;
} {
  let runtime: TestRuntime | undefined;
  beforeAll(async () => {
    runtime = await startPatchedSession(opts, scope);
  });
  afterEach((context) => {
    if (context.task.result?.state === 'fail') runtime?.session.keepDiagnostics();
  });
  afterAll(async () => {
    await runtime?.close();
  });
  return {
    get runtime() {
      if (!runtime) throw new Error('runtime not started');
      return runtime;
    },
  };
}
