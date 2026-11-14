// The emulator of the desktop app behind `EmulatorRuntime` (apps/web/src/runtime/runtime.ts): the controller's session, the pump that steps it and
// the inputs, all through the Host. The browser app has its own, `browserRuntime`, over workers; Console.tsx picks the one of its app.
import type { CardStatus } from '@web/runtime/cards';
import type { ReloadReport } from '@web/runtime/reload';
import type { ControllerJob } from '@web/jobs/jobs-core';
import type {
  EmulatorRuntime,
  NativeAudioStatus,
  RuntimeDiagnostics,
  RuntimeHealth,
  RuntimePatch,
  RuntimeSnapshot,
  RuntimeUpdate,
} from '@web/runtime/runtime';
import { hostMetrics } from '@web/runtime/runtime-metrics';
import type { Host } from './host/host';

/**
 * The emulator of the desktop app, behind `EmulatorRuntime`: every call goes through the Host (`host.emu`, `host.rpc`), which is the Tauri
 * commands in the app and a FakeHost in a test. Resolves with undefined when the page is not inside the app (a plain browser).
 */
export async function createNativeRuntime(
  host: Host,
  onUpdate: (update: RuntimeUpdate) => void,
): Promise<EmulatorRuntime | undefined> {
  if (!host.emu.inApp()) return undefined;
  const metrics = hostMetrics('native_ipc');
  // A refused load leaves the controller's current session intact. Keep enough state to resume that session.
  let accepted: { session: number; snapshot: RuntimeSnapshot; paused: boolean } | undefined;
  let generation = 0,
    activeSession: number | undefined,
    loaded = false,
    paused = true,
    disposed = false,
    epoch = 0,
    timer: ReturnType<typeof setTimeout> | undefined,
    input = Promise.resolve(),
    lifecycle = Promise.resolve();
  const clearPump = () => {
    paused = true;
    epoch += 1;
    metrics.invalidatePumpGap();
    if (timer) globalThis.clearTimeout(timer);
    timer = undefined;
  };
  const fault = (error: unknown) => {
    clearPump();
    loaded = false;
    accepted = undefined;
    onUpdate({ type: 'error', error: String(error) });
  };
  // One protocol method of the controller (docs/CONTROL.md). With a session, the controller refuses the call when the webview's
  // session is no longer the current one; `firmware.load` and `restart` start the session they carry.
  const call = <T>(method: string, params: Record<string, unknown> = {}, session?: number) =>
    host.rpc.call<T>(method, params, session).catch((e: unknown) => {
      // the bridge passes a string like "cancelled (code 10)": the code becomes data, the message loses the suffix
      const m = /^([\s\S]*) \(code (-?\d+)\)$/.exec(e instanceof Error ? e.message : String(e));
      if (!m) throw e;
      throw Object.assign(new Error(m[1]), { code: Number(m[2]) });
    });
  // A new session: stop the pump, run the call that starts it (`firmware.load` or `restart`, with the new token), take the
  // snapshot of what came up, and start pumping it. A newer session wins over an older one that is still starting.
  const newSession = (starts: (token: number) => Promise<void>) => {
    const token = ++generation;
    clearPump();
    loaded = false;
    activeSession = undefined;
    const work = lifecycle
      .catch(() => {})
      .then(async () => {
        await starts(token);
        metrics.reset();
        const snapshot = await host.emu.startup<RuntimeSnapshot>();
        if (disposed) return snapshot;
        accepted = { session: token, snapshot, paused: false };
        if (token !== generation) return snapshot;
        metrics.loaded();
        activeSession = token;
        loaded = true;
        onUpdate({ type: 'snapshot', snapshot });
        start(token);
        return snapshot;
      });
    lifecycle = work.then(
      () => {},
      () => {},
    );
    return work.catch((error) => {
      const refused = String(error).includes('a job is running; wait for it before loading firmware');
      if (!refused) accepted = undefined;
      if (!disposed && token === generation) {
        if (refused && accepted) {
          activeSession = accepted.session;
          loaded = true;
          onUpdate({ type: 'snapshot', snapshot: accepted.snapshot });
          if (!accepted.paused) start(accepted.session);
          onUpdate({ type: 'error', error: String(error) });
        } else fault(error);
      }
      throw error;
    });
  };
  const pump = async (token: number, run: number) => {
    if (disposed || paused || run !== epoch || activeSession !== token) return;
    await input.catch(() => {});
    if (disposed || paused || run !== epoch || activeSession !== token) return;
    try {
      const begin = performance.now();
      const snapshot = await host.emu.step<RuntimeSnapshot>(token, 250_000);
      if (disposed || paused || run !== epoch || activeSession !== token) return;
      if (!snapshot.polled) metrics.step(begin, performance.now(), snapshot);
      if (accepted?.session === token) accepted.snapshot = snapshot;
      onUpdate({ type: 'snapshot', snapshot });
      if (snapshot.status.error) {
        fault(snapshot.status.error);
        return;
      }
      // The controller steps the guest itself: poll once per display frame (a stale callback returns on the epoch check).
      if (snapshot.polled)
        globalThis.requestAnimationFrame(() => {
          void pump(token, run);
        });
      else
        timer = globalThis.setTimeout(() => {
          void pump(token, run);
        }, 0);
    } catch (error) {
      if (activeSession === token && run === epoch) fault(error);
    }
  };
  const start = (token: number) => {
    metrics.invalidatePumpGap();
    paused = false;
    const run = ++epoch;
    timer = globalThis.setTimeout(() => {
      void pump(token, run);
    }, 0);
  };
  const queueInput = (operation: (session: number) => Promise<void>) => {
    const token = generation,
      session = activeSession;
    metrics.invalidatePumpGap();
    const next = input
      .catch(() => {})
      .then(async () => {
        if (!disposed && loaded && token === generation && session === activeSession && session !== undefined)
          await operation(session);
      });
    input = next.then(
      () => {
        if (!disposed && token === generation && session === activeSession) metrics.invalidatePumpGap();
      },
      (error) => {
        if (!disposed && token === generation && session === activeSession) metrics.invalidatePumpGap();
        throw error;
      },
    );
    return input.catch((error) => {
      if (token === generation) fault(error);
      throw error;
    });
  };
  try {
    metrics.reset();
    const startup = await host.emu.startup<RuntimeSnapshot>();
    metrics.loaded();
    generation = 1;
    activeSession = 1;
    loaded = true;
    accepted = { session: 1, snapshot: startup, paused: false };
    onUpdate({ type: 'snapshot', snapshot: startup });
    start(1);
  } catch (error) {
    if (!String(error).includes('No startup firmware selected')) onUpdate({ type: 'error', error: String(error) });
  }
  return {
    saveDiagnostics: (name: string, json: string) => host.emu.saveDiagnostics(name, json),
    cards: {
      reset: () => host.emu.card.reset(),
      stage: (name, bytes) => host.emu.card.stage(name, bytes),
      load: (name) => host.emu.card.load<unknown>(name ?? null),
      status: () => call<CardStatus>('card.status'),
    },
    async diagnostics() {
      const session = activeSession;
      if (session === undefined) throw new Error('No firmware selected');
      const report = await call<RuntimeDiagnostics>('diagnostics', {}, session);
      if (disposed || activeSession !== session) throw new Error('Firmware session changed during diagnostic export');
      return { ...report, host: metrics.report() };
    },
    async nativeAudio() {
      const session = activeSession;
      if (session === undefined) return undefined;
      const report = await host.emu.audioStatus<NativeAudioStatus>(session);
      if (disposed || activeSession !== session) throw new Error('Firmware session changed while reading audio status');
      return (
        report ?? {
          sink_connected: false,
          playback_requested: false,
          flow_started: false,
          source_audio_seconds: 0,
          session_wall_seconds: 0,
          device_error: null,
          setup_needed: true,
        }
      );
    },
    // Open...: the app puts the picked bytes in a file, then `firmware.load` reads it like any other.
    load: (bytes, name, options) =>
      newSession(async (token) => {
        const syx = await host.emu.stageSyx(bytes, name);
        await call('firmware.load', { syx, replace: true, runtime: !!options?.runtime }, token);
      }),
    loadPath: (syx, options) =>
      newSession(async (token) => {
        await call(
          'firmware.load',
          { syx, replace: true, runtime: !!options?.runtime, ...(options?.card ? { card: options.card } : {}) },
          token,
        );
      }),
    async firmwareInfo() {
      const status = await call<{ firmware: { runtime?: RuntimePatch | null } | null }>('status');
      return { runtime: status.firmware?.runtime ?? null };
    },
    async controllerStatus() {
      const status = await call<{ heartbeat?: RuntimeHealth; job?: ControllerJob | null }>('status');
      return { heartbeat: status.heartbeat, job: status.job ?? null };
    },
    cancelJob: async () => {
      await call('job.cancel');
    },
    async restart() {
      if (!loaded || activeSession === undefined) throw new Error('No firmware selected');
      return newSession(async (token) => {
        await call('restart', {}, token);
      });
    },
    // The session stays the same (the call carries its token); the pump stops while the controller loads or restores, a cold boot
    // is a job that refuses steps.
    async reloadModules() {
      const session = activeSession;
      if (!loaded || session === undefined) throw new Error('No firmware selected');
      const resume = !paused;
      clearPump();
      const work = lifecycle
        .catch(() => {})
        .then(async () => {
          try {
            const report = await call<ReloadReport>('dev.reload', { build: true }, session);
            if (disposed || activeSession !== session) return report;
            onUpdate({ type: 'snapshot', snapshot: await host.emu.startup<RuntimeSnapshot>() });
            return report;
          } finally {
            if (!disposed && activeSession === session && resume) start(session);
          }
        });
      lifecycle = work.then(
        () => {},
        () => {},
      );
      return work;
    },
    pause() {
      if (accepted) accepted.paused = true;
      clearPump();
    },
    resume() {
      if (loaded && activeSession !== undefined && paused) {
        if (accepted) accepted.paused = false;
        start(activeSession);
      }
    },
    press: (code) => queueInput((session) => call<void>('input.button', { code, down: true }, session)),
    release: (code) => queueInput((session) => call<void>('input.button', { code, down: false }, session)),
    turn: (encoder, detents) => queueInput((session) => call<void>('input.turn', { encoder, detents }, session)),
    tap: (code, hold) => {
      const token = generation;
      return queueInput(async (session) => {
        const snapshot = await host.emu.tap<RuntimeSnapshot>(session, code, hold);
        if (!disposed && token === generation && activeSession === session) onUpdate({ type: 'snapshot', snapshot });
      });
    },
    async stop() {
      const token = ++generation;
      clearPump();
      loaded = false;
      activeSession = undefined;
      accepted = undefined;
      await (lifecycle = lifecycle.catch(() => {}).then(() => host.emu.stop(token)));
    },
    dispose() {
      disposed = true;
      const token = ++generation;
      clearPump();
      loaded = false;
      activeSession = undefined;
      accepted = undefined;
      void host.emu.stop(token);
    },
  };
}
