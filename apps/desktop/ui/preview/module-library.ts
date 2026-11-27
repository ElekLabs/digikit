// Serve the generated preview module packs through the same library interface as the desktop.
import indexText from '../../../../dist/workbench-preview/index.json?raw';
import type { LibraryHost } from '@digi/workbench-core/host/host.ts';

import { previewModule, type ModuleFixture } from './module-fixture';

interface Pack {
  fixture?: ModuleFixture;
  file: string;
  size: number;
}

interface SiteIndex {
  modules: { id: string; version: string; devices: Record<string, Pack> }[];
  services?: Record<string, Pack[]>;
  ports?: Record<string, Pack[]>;
}

const packUrls = import.meta.glob('../../../../dist/workbench-preview/*/*.{dkm,dks,dkport}', {
  eager: true,
  import: 'default',
  query: '?url',
}) as Record<string, string>;

export function createPreviewLibrary() {
  const index: SiteIndex = JSON.parse(indexText);
  const packs = (device: string) => [
    ...index.modules.flatMap((m) => (m.devices[device] ? [m.devices[device]] : [])),
    ...(index.services?.[device] ?? []),
    ...(index.ports?.[device] ?? []),
  ];
  const fixtures = new Map(
    index.modules.flatMap((module) =>
      Object.values(module.devices).flatMap((pack) =>
        pack.fixture ? [[pack.file, { module, pack, fixture: pack.fixture }] as const] : [],
      ),
    ),
  );
  const bytes = new Map<string, Promise<ArrayBuffer>>();

  const library = {
    async build(device) {
      return { packed: packs(device).length, changed: [], removed: [], failed: [], ms: 0 };
    },
    async scan(device) {
      return packs(device).map((p) => ({ path: p.file, size: p.size }));
    },
    async read(device, path) {
      const simulated = fixtures.get(path);
      if (simulated && simulated.module.devices[device]?.file === path)
        return previewModule(simulated.module, simulated.fixture, simulated.pack.size).buffer;
      const url = packUrls[`../../../../dist/workbench-preview/${path}`];
      if (!url || !packs(device).some((p) => p.file === path))
        throw new Error(`No preview pack for ${device}: ${path}`);
      let pending = bytes.get(path);
      if (!pending) {
        pending = fetch(url).then((response) => {
          if (!response.ok) throw new Error(`Could not read preview pack: ${path}`);
          return response.arrayBuffer();
        });
        bytes.set(path, pending);
        void pending.catch(() => bytes.delete(path));
      }
      return (await pending).slice(0);
    },
    async readMaps() {
      return { ships: [], modules: [] };
    },
  } satisfies LibraryHost;

  return {
    library,
    modules(device: string) {
      return index.modules.flatMap(({ id, devices }) => (devices[device] ? [{ id, pack: devices[device] }] : []));
    },
  };
}

export type PreviewLibrary = ReturnType<typeof createPreviewLibrary>;
