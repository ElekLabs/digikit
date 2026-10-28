// The module set the Patch page adds: the digikit runtime, the `device` set of each device file (devices/*.toml, [patch_sets]). The `device`
// set is the one for a real device (the file is flashed), and `patchkit apply` without --modules uses it; `emulator` is for the emulator and equals it today.
import { parse } from 'smol-toml';
import dt2Source from '../../../../devices/digitakt-ii.toml?raw';
import dn2Source from '../../../../devices/digitone-ii.toml?raw';
import type { DeviceId } from './device-profiles';

function deviceSet(source: string): string[] {
  const set = (parse(source) as { patch_sets?: { device?: unknown } }).patch_sets?.device;
  if (!Array.isArray(set) || set.some((id) => typeof id !== 'string') || set.length === 0)
    throw new Error('a device file has no [patch_sets] device');
  return set as string[];
}

export const runtimeSets: Record<DeviceId, string[]> = { dt2: deviceSet(dt2Source), dn2: deviceSet(dn2Source) };
