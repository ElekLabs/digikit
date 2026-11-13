// The pure part of the "DSP" tick in Console.tsx: which devices have the choice, and what a tick that differs from the running image
// means. Host tests: apps/desktop/ui/tests/dsp.test.ts.
import type { DeviceRecord } from '@digi/devices';

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
