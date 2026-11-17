// The pure part of the "DSP" tick in Console.tsx: which devices have the choice, and what a tick that differs from the running image
// means. Host tests: apps/desktop/ui/tests/dsp.test.ts.
import type { DeviceRecord } from '@digi/devices';
import type { KeyValue } from '@digi/workbench-core/storage.ts';

/** What the controller reports of the DSP choice (`status.dsp_enabled`, `status.dsp_flag`). */
export interface DspState {
  /** The choice the running image was started with. */
  enabled: boolean;
  /** The launcher's `--no-dsp` / `--dsp-on`, when it gave one: the webview adopts it instead of its saved choice. */
  flag: boolean | null;
}

/** A device has the choice when it has a DSP, or a second CPU that runs a program and has a stand-in (the device file's `cf2` section). */
export const hasDspChoice = (d: Pick<DeviceRecord, 'capabilities' | 'container'>): boolean =>
  d.capabilities.dsp === 'sharc' || d.container.cf2 !== undefined;

/** The tick differs from the running image: it applies at the next Reload or Open. */
export const dspPending = (tick: boolean, running: DspState | undefined): boolean =>
  running !== undefined && running.enabled !== tick;

/** The tick the window starts with: the launcher's flag wins over the saved choice. */
export const startupDsp = (saved: boolean, running: DspState | undefined): boolean => running?.flag ?? saved;

/** The Metrics line. */
export const dspLine = (running: DspState | undefined): string =>
  running === undefined ? '—' : running.enabled ? 'DSP on' : 'DSP off';

/** The saved "DSP" choice: on unless the user turned it off. A storage that cannot be read gives "on". */
export const DSP_KEY = 'digikit.bootDsp';
export const readDspChoice = (storage: Pick<KeyValue, 'get'>): boolean => {
  try {
    return storage.get(DSP_KEY) !== 'off';
  } catch {
    return true;
  }
};
/** Keep the choice. Only a click on the tick calls this: the launcher's one-time flag lasts for the session and is never saved. */
export const writeDspChoice = (storage: Pick<KeyValue, 'set'>, on: boolean): void => {
  try {
    storage.set(DSP_KEY, on ? 'on' : 'off');
  } catch {
    /* the choice lasts until the window closes */
  }
};

/**
 * The `dsp` option of a load: the tick when the image's device offers the choice (`undefined` device: unknown, the tick applies),
 * else left out so the controller keeps its own rule for that device.
 */
export const dspOption = (
  device: Pick<DeviceRecord, 'capabilities' | 'container'> | undefined,
  tick: boolean,
): boolean | undefined => (device === undefined || hasDspChoice(device) ? tick : undefined);
