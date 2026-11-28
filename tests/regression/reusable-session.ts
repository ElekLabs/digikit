import fs from 'node:fs';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, beforeEach } from 'vitest';
import { DkSession, EmuUsbTransport, type EmuRpcCall } from '@digi/dk-session';
import { Session, withinRuntimeDeadline, type RuntimeBaseline, type SessionOptions } from '@digi/test-support';
import { aggregate } from './aggregate-error.ts';
import { fileTemp } from './file-temp.ts';
import { emulatedOptions } from './runtime/waits.ts';
import { removeEntry, type RunDirClaim } from './runtime/run-claim.ts';
import type { RuntimeCase } from './test-runtime.ts';
import { recordPhase, setTimingContext, timePhase } from './timing.ts';
import { startNoProgressWatchdog, type Watchdog } from './watchdog.ts';
import { withWaitApi } from './wait-api.ts';

/** Opt-in fixture: independent cases reload pristine firmware/card state in one owned daemon. */
export function withReusablePatchedSession(
  opts: Omit<SessionOptions, 'runtime' | 'dsp'> & {
    patchSet?: string;
    dsp?: 'zero' | 'sharc' | 'cold';
    /** Called when the daemon starts: a persistent run dir for it (see `claimRunDir`), or undefined for a fresh temporary one. */
    runDirClaim?: () => RunDirClaim | undefined;
    /** Wraps the rpc of each case's protocol session (a card rig refuses calls its paused daemon cannot answer). */
    protocolCall?: (call: EmuRpcCall, session: Session) => EmuRpcCall;
  },
  lifecycle: {
    prepareBaseline: (session: Session) => Promise<RuntimeBaseline>;
    /** Required scenario evidence (guard/fault/card/PCM) before state is discarded. Holds release in their own finally. */
    finishCase: (runtime: RuntimeCase) => Promise<void>;
    /** Validate process-lifetime artifacts only after the owned daemon has closed, even on a failed case. */
    afterClose?: (session: Session) => Promise<void>;
    /** Temporary directories of the fixture (artifacts): removed when the file ends clean, kept (and printed) when it failed. */
    tempDirs?: () => string[];
  },
  scope = 'reused runtime',
): { readonly runtime: RuntimeCase; reload(baseline: RuntimeBaseline): Promise<RuntimeCase> } {
  let session: Session | undefined;
  let watchdog: Watchdog | undefined;
  let baseline: RuntimeBaseline | undefined;
  let runtime: RuntimeCase | undefined;
  let invalidate: (() => void) | undefined;
  let transitioning = false;
  let claim: RunDirClaim | undefined;
  /** A failed file leaves a run dir that must not be reused: a snapshot of a boot that failed a check would pass the next run. */
  let failed = false;
  /** Set when a failed test closed the session: the next test starts a new one. */
  let needsFreshSession = false;
  const releaseClaim = () => {
    // A snapshot that a failed file's boots wrote is not reused: its state may be one that a check rejected.
    if (failed) for (const entry of session?.writtenSnapshots ?? []) removeEntry(entry);
    claim?.release(failed);
    claim = undefined;
  };
  const disposeTemp = () => {
    for (const dir of lifecycle.tempDirs?.() ?? []) if (dir) fileTemp(dir, failed);
  };
  const loadCase = async (next: RuntimeBaseline): Promise<RuntimeCase> => {
    const context = { scope, device: opts.device, session: randomUUID(), dsp: session!.dsp };
    setTimingContext(session!, context);
    const epoch = await timePhase(
      'session.baseline-reload',
      async () => {
        const result = await session!.reloadRuntimeBaseline(next);
        // Child records: the firmware.load call (a boot snapshot hit or a cold boot) and the rest (status, guard, ready checks).
        const reload = session!.lastReload;
        const load = session!.loadResult;
        if (reload) {
          recordPhase('reload.firmware-load', reload.loadMs, {
            startUsed: load.start_used,
            cacheHit: load.cache_hit === true,
            cacheDir: load.cache_dir,
            card: path.basename(path.dirname(next.cardImage)),
          });
          recordPhase('reload.other', reload.totalMs - reload.loadMs);
        }
        return result;
      },
      context,
    );
    let active = true;
    const raw: EmuRpcCall = async <T>(method: string, params?: Record<string, unknown>) => {
      if (!active) throw new Error('runtime case protocol is no longer active');
      const result = await session!.client.call<T>(method, params);
      if (!active) throw new Error('runtime case protocol is no longer active');
      return result;
    };
    const dk = new DkSession(
      new EmuUsbTransport(opts.protocolCall ? opts.protocolCall(raw, session!) : raw),
      emulatedOptions(session!),
    );
    const invalidateCase = () => {
      active = false;
      dk.cancelPending();
    };
    invalidate = invalidateCase;
    runtime = {
      ...withWaitApi({
        session: session!,
        dk,
        close: async () => {
          if (!active) throw new Error('runtime case is no longer active');
          invalidateCase();
          await session!.close();
        },
      }),
      epoch,
    };
    return runtime;
  };
  const finishCurrentCase = async (): Promise<void> => {
    if (!runtime) return;
    const current = runtime;
    const cancel = invalidate;
    const deadline = Date.now() + 10_000;
    const errors: unknown[] = [];
    try {
      await withinRuntimeDeadline(deadline, () => lifecycle.finishCase(current));
    } catch (error) {
      errors.push(error);
    }
    try {
      // Without a DSP (zero or cold ColdFire-only) there is no audio capture to stop.
      if (session!.dsp === 'sharc')
        await withinRuntimeDeadline(deadline, () => session!.client.call('pcm.tap', { action: 'capture_stop' }));
      await withinRuntimeDeadline(deadline, () => session!.client.call('run.mode', { mode: 'paused' }));
    } catch (error) {
      errors.push(error);
    } finally {
      // No release-all RPC exists. Bounded taps/chords own release; the mandatory next load clears runtime/scheduled input.
      cancel?.();
      runtime = undefined;
    }
    if (errors.length) {
      failed = true;
      try {
        fs.writeFileSync(
          path.join(session!.runDir, 'runtime-case-error.json'),
          JSON.stringify(errors.map(String), null, 2),
        );
      } catch {
        /* Preserve evidence/cleanup errors if diagnostics cannot be written. */
      }
      await session!.close({ preserveDiagnostics: true });
      throw aggregate(errors, 'runtime case evidence or cleanup failed; owned daemon discarded');
    }
  };
  const startSession = async (): Promise<void> => {
    const { runDirClaim: _claim, protocolCall: _wrap, ...sessionOptions } = opts;
    session = await Session.start({
      ...sessionOptions,
      onSession: (started) => {
        watchdog = startNoProgressWatchdog(started);
      },
      ...(claim ? { runDir: claim.dir, snapshotDir: claim.snapshotDir } : {}),
      runtime: { patchSet: opts.patchSet, reuse: true },
    });
  };
  beforeAll(async () => {
    claim = opts.runDirClaim?.();
    try {
      await startSession();
    } catch (error) {
      watchdog?.stop();
      failed = true;
      releaseClaim();
      throw error;
    }
    try {
      baseline = await lifecycle.prepareBaseline(session!);
    } catch (error) {
      watchdog?.stop();
      failed = true;
      await session!.close({ preserveDiagnostics: true });
      releaseClaim();
      throw error;
    }
  });
  beforeEach(async () => {
    if (needsFreshSession) {
      // The previous test failed: its daemon is closed. A failure stays local to that test; this one boots a new session.
      watchdog?.stop();
      await startSession();
      baseline = await lifecycle.prepareBaseline(session!);
      needsFreshSession = false;
    }
    await loadCase(baseline!);
  });
  afterEach(async (context) => {
    await finishCurrentCase();
    if (context.task.result?.state === 'fail') {
      failed = true;
      needsFreshSession = true;
      watchdog?.stop();
      // Snapshots of the failed session must not be reused; releaseClaim only sees the last session.
      for (const entry of session?.writtenSnapshots ?? []) removeEntry(entry);
      await session?.close({ preserveDiagnostics: true });
    }
  });
  let checkedClose = false;
  afterAll(async () => {
    watchdog?.stop();
    invalidate?.();
    if (!session || checkedClose) {
      releaseClaim();
      return disposeTemp();
    }
    checkedClose = true;
    const errors: unknown[] = [];
    try {
      await session.close();
    } catch (error) {
      errors.push(error);
    }
    try {
      await lifecycle.afterClose?.(session);
    } catch (error) {
      errors.push(error);
    }
    if (errors.length) failed = true;
    releaseClaim();
    disposeTemp();
    if (errors.length) throw aggregate(errors, 'runtime shutdown or post-close evidence failed');
  });
  return {
    get runtime() {
      if (!runtime) throw new Error('runtime case not started');
      return runtime;
    },
    async reload(next) {
      if (!runtime || transitioning) throw new Error('runtime case not active or already transitioning');
      transitioning = true;
      try {
        await finishCurrentCase();
        return await loadCase(next);
      } finally {
        transitioning = false;
      }
    },
  };
}
