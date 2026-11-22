// The names of the runtime log over the Host. One path for both origins: the load table is read through PEEK of the debug module on the session
// the follower already uses (the emulator's USB capture or the device's MIDI port), and dkprof.wasm names the addresses. Only the source of the
// ship map differs:
//   emulator  the controller call `runtime.maps` (the live image; it follows `dev.reload`)
//   device    the ship map `firmware_prepare` kept for the image id the device reports in INFO (`library.readMaps`)
// Maps of the built modules come from `library.readMaps` for both.
import type { Host } from '@digi/workbench-core/host/host.ts';
import type { DkProf } from '@digi/dk-session/dkprof';
import { createLogNames, type LogNames, type NameSources, type RuntimeMaps } from '@digi/dk-session/log-names';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { stockImageHash, type NameSession, type ShipSource } from '@digi/dk-session/log-names';
import { deviceOfPort } from './remote-core';

type SymbolHost = Pick<Host, 'library' | 'symbols'>;

const sources = (host: SymbolHost, prof: () => Promise<DkProf>): Omit<NameSources, 'ship'> => ({
  prof,
  moduleMaps: async (device) => (await host.library.readMaps(device)).modules.map((m) => m.json),
  firmwareMaps: host.symbols?.maps,
  revision: host.symbols?.revision,
});

async function emulatorShip(host: Pick<Host, 'rpc'>): Promise<ShipSource> {
  const maps = await host.rpc.call<RuntimeMaps>('runtime.maps');
  return { ship: maps.ship, symbols: maps.symbols, matches: maps.matches };
}

async function deviceShip(host: SymbolHost, session: NameSession): Promise<ShipSource | { unavailable: string }> {
  const info = await session.info();
  const device = deviceOfPort(info.port ?? '');
  if (!device || !info.image_id)
    return { unavailable: 'the device does not report its image id (flash it from the Firmware page)' };
  const record = (await host.library.readMaps(device)).ships.find((s) => s.image_id === info.image_id);
  return record
    ? { ship: record.ship, symbols: record.symbols }
    : { unavailable: `no map for the image ${info.image_id} (flash it from the Firmware page of this workbench)` };
}

export async function selectedSymbolImage(host: Host, source: TargetBinding): Promise<string | undefined> {
  source.check();
  if (source.target.kind === 'none') return;
  const image =
    source.target.kind === 'emulator'
      ? await emulatorShip(host).catch(() => undefined)
      : await deviceShip(host, await source.session());
  source.check();
  if (image) return 'ship' in image && image.matches !== false ? stockImageHash(JSON.parse(image.ship)) : undefined;

  // An unpatched emulator has no ship map. Use the exact loaded bytes for the Settings SHA match.
  const { firmware } = await host.rpc.call<{ firmware?: { syx_sha256?: string } | null }>('status');
  source.check();
  return stockImageHash({ key: { image_sha256: firmware?.syx_sha256 } });
}

export function createEmulatorNames(host: SymbolHost & Pick<Host, 'rpc'>, prof: () => Promise<DkProf>): LogNames {
  return createLogNames({ ...sources(host, prof), ship: () => emulatorShip(host) });
}

export function createDeviceNames(host: SymbolHost, prof: () => Promise<DkProf>): LogNames {
  return createLogNames({ ...sources(host, prof), ship: (session) => deviceShip(host, session) });
}
