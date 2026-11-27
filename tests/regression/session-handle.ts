import { afterAll, afterEach, beforeAll, beforeEach } from 'vitest';
import { Session, type SessionOptions } from '@digi/test-support';
import { daemonBinary } from './runtime/devices.ts';

export interface SessionHandle {
  /** The session of this test file; valid inside tests and hooks. */
  readonly session: Session;
}

/** One daemon per test file: started in beforeAll, closed in afterAll. Unless `restart: false`, each test
 * starts from `session.restart()`. */
export function withSession(opts: SessionOptions & { restart?: boolean }): SessionHandle {
  let session: Session | undefined;
  beforeAll(async () => {
    // Without an explicit binary or DIGI_EMUD, take the daemon from DIGI_EMUD_<DEV> or dist/daemons.json.
    const binary = opts.binary ?? (process.env.DIGI_EMUD ? undefined : daemonBinary(opts.device));
    session = await Session.start({ ...opts, binary });
  });
  afterEach((context) => {
    if (context.task.result?.state === 'fail') session?.keepDiagnostics();
  });
  afterAll(async () => {
    await session?.close();
  });
  beforeEach(async () => {
    if (opts.restart !== false) await session!.restart();
  });
  return {
    get session() {
      if (!session) throw new Error('session not started');
      return session;
    },
  };
}
