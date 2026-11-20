import path from 'node:path';

/** The devices whose stock firmware the tools read, and the analysis version of each (AGENTS.md: DT2 1.17, DN2 1.12). */
export type StockDevice = 'dt2' | 'dn2';

export const STOCK_SYX: Record<StockDevice, string> = {
  dt2: 'Digitakt_II_OS1.17.syx',
  dn2: 'Digitone_II_OS1.12.syx',
};

/** The stock images: $DIGI_FIRMWARE_DIR (relative to the checkout `root`), else `firmware/`. The same rule as tools/firmware/config.py FIRMWARE_DIR, the control daemon and tools/nx/device-inputs.mjs. */
export function firmwareDir(root: string, env: NodeJS.ProcessEnv = process.env): string {
  const dir = env.DIGI_FIRMWARE_DIR;
  return dir ? path.resolve(root, dir) : path.join(root, 'firmware');
}

/** The path of the stock .syx of a device (the file may not exist). */
export function stockImage(device: string, root: string, env: NodeJS.ProcessEnv = process.env): string {
  const name = STOCK_SYX[device as StockDevice];
  if (!name) throw new Error(`no stock firmware is named for the device ${JSON.stringify(device)}`);
  return path.join(firmwareDir(root, env), name);
}
