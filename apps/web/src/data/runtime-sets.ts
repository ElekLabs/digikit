// The module set the Patch page adds: the digikit runtime, the `device` set of each device file (devices/*.toml, [patch_sets]). The `device`
// set is the one for a real device (the file is flashed), and `patchkit apply` without --modules uses it; `emulator` is for the emulator and equals it today.
import { registry, runtimeDeviceIds, type DeviceId } from './device-profiles';

function deviceSet(id: DeviceId): string[] {
  const set = registry.get(id).patchSets.device;
  if (!set || set.length === 0) throw new Error(`device file of ${id} has no [patch_sets] device`);
  return [...set];
}

export const runtimeSets: Record<DeviceId, string[]> = Object.fromEntries(
  runtimeDeviceIds.map((id) => [id, deviceSet(id)]),
);
