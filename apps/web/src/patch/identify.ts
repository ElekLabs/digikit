import { deviceNameOf, deviceProfiles, registry, type DeviceId } from '../data/device-profiles';
import { sha256Hex } from '../runtime/local-inputs';

export interface Identity {
  device: DeviceId;
  deviceName: string;
  version: string;
}

/** Which stock firmware a SHA-256 is, from the device profiles (undefined for anything else, including patched images). */
export function identify(sha256: string): Identity | undefined {
  const sha = sha256.toLowerCase();
  for (const profile of Object.values(deviceProfiles)) {
    const match = profile.firmware.find((f) => f.sha256 === sha);
    if (match) return { device: profile.short, deviceName: profile.name, version: match.version };
  }
  return undefined;
}

export const deviceName = (device: string): string => deviceNameOf(device);

/** The device byte of a framed Elektron .syx (`F0 00 20 3C <id>`) as a device id, from `[device] syx_id`; undefined for anything else. */
export function deviceOfSyx(bytes: Uint8Array): DeviceId | undefined {
  if (bytes.length < 5 || bytes[0] !== 0xf0 || bytes[1] !== 0x00 || bytes[2] !== 0x20 || bytes[3] !== 0x3c)
    return undefined;
  return registry.devices.find((d) => d.syxId === bytes[4])?.id;
}

/**
 * The device a firmware image is for: a stock image by its SHA-256 against the registry's firmware rows, a patched image by the device
 * byte of its framing. Never by the file name.
 */
export async function deviceOfImage(bytes: Uint8Array): Promise<DeviceId | undefined> {
  return identify(await sha256Hex(bytes))?.device ?? deviceOfSyx(bytes);
}
