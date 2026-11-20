// The state of the Default modules list for the life of the app (`createDefaultModules`): which entries the user unticked (kept in the app's storage), and the run that
// follows a flash (the part of the Firmware page that shows it is FirmwarePage.tsx; the emulator's run is Console.tsx). The rules are in
// default-modules-core.ts; the app gives the entries and the install through `FirmwareEnv.defaultModules`.
import { createSignal } from 'solid-js';
import {
  DefaultModuleError,
  parseUnticked,
  patchSetFor,
  tickedIds,
  withTick,
  type DefaultModuleEntry,
  type Unticked,
} from './default-modules-core';
import { remembered, type KeyValue } from '../storage';
import type { FirmwareEnv } from './env';

/** The run of the default modules after a flash. */
export type PostRun =
  | { kind: 'idle' }
  | { kind: 'working'; text: string; done: number; total: number }
  | { kind: 'done'; lines: string[] }
  | { kind: 'failed'; message: string };

const KEY = 'digikit.defaultModules.unticked';
const BOOT_KEY = 'digikit.customBootScreen';

export function createDefaultModules(env: FirmwareEnv, storage: KeyValue) {
  const readUnticked = (): Unticked => {
    try {
      return parseUnticked(storage.get(KEY));
    } catch {
      return {};
    }
  };

  const [unticked, setUnticked] = createSignal<Unticked>(readUnticked());

  /** The Custom boot screen toggle (core/introblank in the patch roots), off by default; kept in the storage. */
  const [bootValue, setBootValue] = remembered<'0' | '1'>(storage, BOOT_KEY, (v) => (v === '1' ? '1' : '0'));
  const customBootScreen = (): boolean => bootValue() === '1';
  const setCustomBootScreen = (on: boolean): void => setBootValue(on ? '1' : '0');

  /** The patch set the page prepares: `device`, or `device-introblank` with the toggle on. */
  const patchSet = (): string => patchSetFor(customBootScreen());

  function setTick(device: string, id: string, on: boolean): void {
    const next = withTick(unticked(), device, id, on);
    setUnticked(next);
    storage.set(KEY, JSON.stringify(next));
  }

  /** The list for a device, or none where the app has no default modules (the site). */
  const entriesFor = (device: string): DefaultModuleEntry[] => env.defaultModules?.entries(device) ?? [];

  /** The ids ticked for the device, in list order. */
  const tickedFor = (device: string): string[] => tickedIds(entriesFor(device), unticked(), device);

  const [postRun, setPostRun] = createSignal<PostRun>({ kind: 'idle' });
  const postWorking = (): boolean => postRun().kind === 'working';

  /**
   * After a finished flash: install and enable the ticked default modules of the device on the flashed device. One run, no question: the flash
   * stays done when a module fails, and the failed module is named. Does nothing where the app has no default modules or nothing is ticked.
   */
  async function installAfterFlash(device: string): Promise<void> {
    const ids = tickedFor(device);
    if (!env.defaultModules || ids.length === 0) return;
    setPostRun({ kind: 'working', text: 'Starting the default modules', done: 0, total: ids.length + 2 });
    env.log({ level: 'info', text: `default modules for ${device}: ${ids.join(', ')}` });
    try {
      const lines = await env.defaultModules.install('device', device, ids, (p) =>
        setPostRun({ kind: 'working', ...p }),
      );
      setPostRun({ kind: 'done', lines });
      env.log({ level: 'info', text: 'default modules installed', detail: lines.join('\n') });
    } catch (e) {
      const message =
        e instanceof DefaultModuleError
          ? `Default module ${e.message}. The firmware flash is done; install it on the Transfer page.`
          : `Default modules: ${e instanceof Error ? e.message : String(e)}. The firmware flash is done.`;
      setPostRun({ kind: 'failed', message });
      env.log({ level: 'error', text: message });
    }
  }

  const resetPostRun = (): void => {
    if (!postWorking()) setPostRun({ kind: 'idle' });
  };

  return {
    unticked,
    customBootScreen,
    setCustomBootScreen,
    patchSet,
    setTick,
    entriesFor,
    tickedFor,
    postRun,
    postWorking,
    installAfterFlash,
    resetPostRun,
  };
}
export type DefaultModulesApp = ReturnType<typeof createDefaultModules>;
