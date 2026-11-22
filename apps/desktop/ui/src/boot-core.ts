// The pure part of the loading state in Console.tsx: what the status bar and the Log say while a firmware starts.
// Host tests: apps/desktop/ui/tests/boot.test.ts.
import { deviceNameOf, type DeviceId } from '@digi/workbench-core/data/device-profiles.ts';
import type { Target } from '@digi/workbench-core/transfer/target.ts';

export type BootDevice = DeviceId;
/** A firmware that is starting: its file name, and the device when the image tells it (`deviceOfImage`). */
export type Booting = { name: string; device?: BootDevice };

export function booting(name: string, device?: BootDevice): Booting {
  return device === undefined ? { name } : { name, device };
}

/** The status bar text: the device name once known, else the file name, else a bare `Booting…`. */
export function bootingStatus(b: Booting): string {
  const what = b.device ? deviceNameOf(b.device) : b.name;
  return what ? `Booting ${what}…` : 'Booting…';
}

/** The kind of the target; only `emulator` is the emulator. */
export type TargetKind = Target['kind'];

/**
 * Does the app boot a firmware at once when it starts? A command-line file always does. The last firmware of "Load on start" only when the target is
 * the emulator: with a device as target the emulator waits for the first visit of its page (`bootOnEmulatorPage`).
 */
export function shouldBootOnStart(
  launchSyx: string | null | undefined,
  loadOnStart: boolean,
  targetKind: TargetKind,
): boolean {
  return !!launchSyx || (loadOnStart && targetKind === 'emulator');
}

/** After the start deferred the boot of the last firmware: boot it when the Emulator page is shown with the emulator as target and nothing runs. */
export function bootOnEmulatorPage(
  pending: boolean,
  page: string,
  targetKind: TargetKind,
  running: boolean,
  starting: boolean,
): boolean {
  return pending && page === 'run' && targetKind === 'emulator' && !running && !starting;
}

/**
 * Does a deferred start boot stay pending? Not once a firmware runs or starts (by hand, by a file or a command-line launch): the last firmware must not
 * boot later over an unloaded or failed one.
 */
export function keepDeferredBoot(running: boolean, starting: boolean): boolean {
  return !running && !starting;
}

/** The Log entry when a boot starts. */
export function bootingScreen(b: Booting): string {
  return `${bootingStatus(b)} the first boot of a firmware can take a minute`;
}

/** The status bar text for a failed load. */
export function loadFailure(name: string, error: string): string {
  return `Loading ${name || 'the firmware'} failed: ${error}`;
}
