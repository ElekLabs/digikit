import { deviceNameOf, deviceProfiles, type DeviceId } from '../data/device-profiles';

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
