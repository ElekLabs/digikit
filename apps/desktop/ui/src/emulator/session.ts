import { createSignal, onCleanup, onMount } from 'solid-js';
import type { EmuLaunch, Host } from '@digi/workbench-core/host/host.ts';
import type { EmulatorRuntime, RuntimeUpdate } from '@digi/workbench-core/runtime/runtime.ts';
import { createNativeRuntime } from '../native-runtime';
import type { BuildInfo } from '../pgo-warning';

type SessionOptions = {
  host: Pick<Host, 'emu' | 'rpc'>;
  update: (update: RuntimeUpdate) => void;
  applyLaunch: (launch: EmuLaunch) => Promise<void>;
  pollSource: () => Promise<void>;
  pollAudio: () => Promise<void>;
  pollHealth: () => Promise<void>;
};

export type SessionRuntime = Omit<EmulatorRuntime, 'stop' | 'dispose'>;

/** Under the Workbench window's Solid owner, keep the native runtime and its reads alive across page changes. */
export function createEmulatorSession(options: SessionOptions) {
  const [runtime, setRuntime] = createSignal<EmulatorRuntime>();
  const [outsideApp, setOutsideApp] = createSignal(false);
  const [buildInfo, setBuildInfo] = createSignal<BuildInfo>();
  const lifetime = new AbortController();
  const { signal } = lifetime;
  const timers: ReturnType<typeof setInterval>[] = [];

  const update = (event: RuntimeUpdate) => {
    if (!signal.aborted) options.update(event);
  };
  const applyLaunch = async () => {
    try {
      const launch = await options.host.emu.launch();
      if (!signal.aborted) await options.applyLaunch(launch);
    } catch (error) {
      update({ type: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  };
  const start = async () => {
    const r = await createNativeRuntime(options.host, update, signal);
    if (signal.aborted) {
      r?.dispose();
      return;
    }
    if (!r) {
      setOutsideApp(true);
      return;
    }
    setRuntime(r);
    if (signal.aborted) return;
    void options.host.emu.buildInfo<BuildInfo>().then(
      (info) => {
        if (!signal.aborted) setBuildInfo(info);
      },
      () => {
        if (!signal.aborted) setBuildInfo(undefined);
      },
    );
    void applyLaunch();
    timers.push(globalThis.setInterval(() => void options.pollSource(), 1000));
    timers.push(globalThis.setInterval(() => void options.pollAudio(), 1000));
    timers.push(globalThis.setInterval(() => void options.pollHealth(), 1000));
  };

  onCleanup(() => {
    lifetime.abort();
    timers.forEach((t) => globalThis.clearInterval(t));
    const r = runtime();
    setRuntime(undefined);
    r?.dispose();
  });
  onMount(() => void start());

  return { runtime: (): SessionRuntime | undefined => runtime(), outsideApp, buildInfo, signal };
}
