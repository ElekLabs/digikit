// The pure part of the loading state in Console.tsx: what the status bar and the Log say while a firmware starts.
// Host tests: apps/desktop/ui/tests/boot.test.ts.
import { deviceNameOf, type DeviceId } from '../../../web/src/data/device-profiles';

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

/** The Log entry when a boot starts. */
export function bootingScreen(b: Booting): string {
  return `${bootingStatus(b)} the first boot of a firmware can take a minute`;
}

/** The status bar text for a failed load. */
export function loadFailure(name: string, error: string): string {
  return `Loading ${name || 'the firmware'} failed: ${error}`;
}
