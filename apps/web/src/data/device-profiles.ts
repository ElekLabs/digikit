// The devices the site knows, from the registry (devices/*.toml through @digi/devices): names, firmware identities and the
// short ids. No device is named in code; a device file added to devices/ shows up here.
import { registry } from '../../../../packages/devices/src/vite.ts';

/** A device id: the `[device] short` of a device file. */
export type DeviceId = string;
export type BootContract = 'main-os-oracle-v1';
export type SymbolProfile = 'elektron-rtos-v1';
export type FirmwareIdentity = Readonly<{
  version: string;
  sha256: string;
  filename?: string;
  main_sha256?: string;
  boot_contract?: BootContract;
  symbol_profile?: SymbolProfile;
}>;
export type DeviceProfile = Readonly<{ name: string; short: DeviceId; firmware: readonly FirmwareIdentity[] }>;

export { registry };

/** Every device of the registry, in file-name order. */
export const deviceIds: readonly DeviceId[] = registry.ids;

/** The devices that run the digikit runtime: the ones with module lists and runtime sets. */
export const runtimeDeviceIds: readonly DeviceId[] = registry.runtimeIds;

/** The registry's name of a device (`[device] name`), or the id itself when the id is not a device. */
export const deviceNameOf = (id: string): string => (deviceIds.includes(id) ? registry.get(id).name : id);

/** The short label of a device on a card (the id in upper case). */
export const deviceShort = (id: string): string => id.toUpperCase();

/** The names of the runtime devices as prose: "A and B" (`and`) or "A or B" (`or`). */
export function deviceNamesText(conj: 'and' | 'or'): string {
  const names = runtimeDeviceIds.map(deviceNameOf);
  return names.length < 2 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} ${conj} ${names[names.length - 1]}`;
}

/** The newest firmware release of each runtime device that the site can patch (it has a boot profile), as prose: "A OS 1.2 or B OS 3.4". */
export function newestReleasesText(): string {
  return runtimeDeviceIds
    .flatMap((id) => {
      const newest = deviceProfiles[id]?.firmware.filter((f) => f.main_sha256).at(-1);
      return newest ? [`${deviceNameOf(id)} OS ${newest.version}`] : [];
    })
    .join(' or ');
}

/** Is `id` a device of the registry? */
export const isDeviceId = (id: string): boolean => deviceIds.includes(id);

/** Which device a MIDI port name belongs to: the first word of a device name in the port name ("Elektron A II" is the device named "A II"). */
export function deviceOfPort(port: string): DeviceId | undefined {
  const p = port.toLowerCase();
  return registry.devices.find((d) =>
    new RegExp(`(?<![a-z0-9])${d.name.split(/\s+/)[0]?.toLowerCase()}(?![a-z0-9])`).test(p),
  )?.id;
}

const BOOT_CONTRACT = 'main-os-oracle-v1';
const SYMBOL_PROFILE = 'elektron-rtos-v1';

function profile(id: DeviceId): DeviceProfile {
  const d = registry.get(id);
  const firmware = d.firmware.map((f): FirmwareIdentity => {
    if (f.boot && (f.boot.contract !== BOOT_CONTRACT || f.boot.symbolProfile !== SYMBOL_PROFILE))
      throw new Error('device profile boot fields must be a supported complete contract');
    return {
      version: f.version,
      sha256: f.sha256,
      ...(f.filename === undefined ? {} : { filename: f.filename }),
      ...(f.boot
        ? { main_sha256: f.boot.mainSha256, boot_contract: BOOT_CONTRACT, symbol_profile: SYMBOL_PROFILE }
        : {}),
    } as FirmwareIdentity;
  });
  return { name: d.name, short: id, firmware };
}

export const deviceProfiles: Readonly<Record<DeviceId, DeviceProfile>> = Object.fromEntries(
  deviceIds.map((id) => [id, profile(id)]),
);

/** The DN2 firmware the private ready inputs (dev server, `/__digi-local/`) were captured from; empty when the registry lacks it. */
export const localReadyFirmwareSha256 =
  deviceProfiles.dn2?.firmware.find((firmware) => firmware.version === '1.11')?.sha256 ?? '';

const allHashes = new Set<string>();
for (const device of Object.values(deviceProfiles))
  for (const firmware of device.firmware) {
    if (allHashes.has(firmware.sha256)) throw new Error(`duplicate firmware SHA-256 ${firmware.sha256}`);
    allHashes.add(firmware.sha256);
  }
