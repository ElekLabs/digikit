import path from 'node:path';
import { stockFirmware, type Device, type Registry } from '@digi/devices';
import { registry } from '@digi/devices/node';

/**
 * The file name of the stock .syx of a device: the `filename` of the firmware that `stockFirmware` picks in its device file (devices/*.toml),
 * so a device needs no entry here. Throws when the device is unknown or its file names no firmware file.
 */
export function stockSyx(device: string, reg: Registry = registry()): string {
  const name = reg.ids.includes(device as Device) ? stockFirmware(reg.get(device))?.filename : undefined;
  if (!name) throw new Error(`no stock firmware is named for the device ${JSON.stringify(device)}`);
  return name;
}

/** The stock images: $DIGI_FIRMWARE_DIR (relative to the checkout `root`), else `firmware/`. The same rule as tools/firmware/config.py FIRMWARE_DIR, the control daemon and tools/nx/device-inputs.mjs. */
export function firmwareDir(root: string, env: NodeJS.ProcessEnv = process.env): string {
  const dir = env.DIGI_FIRMWARE_DIR;
  return dir ? path.resolve(root, dir) : path.join(root, 'firmware');
}

/** The path of the stock .syx of a device (the file may not exist). */
export function stockImage(device: string, root: string, env: NodeJS.ProcessEnv = process.env, reg?: Registry): string {
  return path.join(firmwareDir(root, env), stockSyx(device, reg));
}
