// Keep user-selected paths in preferences. Firmware-derived names live in this app instance only.
import type { SymbolImageMap, SymbolKind, SymbolSource, SymbolsHost } from '@digi/workbench-core/host/host.ts';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import type { AddrMapJson, Cpu } from '@digi/dk-session/dkprof';

interface SymbolFiles {
  pick(kind: SymbolKind): Promise<string | null>;
  read(kind: SymbolKind, path: string): Promise<SymbolImageMap[]>;
}

type Selection = Pick<SymbolSource, 'kind' | 'path'>;
interface Loaded {
  source: SymbolSource;
  images: Map<string, Partial<Record<Cpu, AddrMapJson>>>;
}

const KEY = 'digikit.firmwareSymbols.v1';

function selections(storage: KeyValue): Selection[] {
  try {
    const values: unknown = JSON.parse(storage.get(KEY) ?? '[]');
    if (!Array.isArray(values)) return [];
    return values.filter((v) => v && typeof v.path === 'string' && ['cf', 'sharc', 'map'].includes(v.kind));
  } catch {
    return [];
  }
}

export function createFirmwareSymbols(storage: KeyValue, files: SymbolFiles): SymbolsHost {
  let loaded: Loaded[] = [];
  let initialized = false;
  let revision = 0;
  let queue = Promise.resolve();

  async function load({ kind, path }: Selection): Promise<Loaded> {
    const source: SymbolSource = { kind, path, images: [] };
    const images: Loaded['images'] = new Map();
    try {
      for (const { json, ...identity } of await files.read(kind, path)) {
        const map = JSON.parse(json);
        const channels: Partial<Record<Cpu, AddrMapJson>> = {};
        for (const cpu of ['cf', 'sharc'] as const) {
          const entries = map.entries.filter((entry: { cpu: Cpu }) => entry.cpu === cpu);
          if (entries.length) channels[cpu] = { ...map, entries };
        }
        images.set(identity.sha256, channels);
        source.images.push(identity);
      }
    } catch (error) {
      source.error = error instanceof Error ? error.message : String(error);
      source.images = [];
      images.clear();
    }
    return { source, images };
  }

  async function reload() {
    const next: Loaded[] = [];
    for (const selection of selections(storage)) next.push(await load(selection));
    loaded = next;
    initialized = true;
    revision++;
  }

  function save(next: Loaded[]) {
    storage.set(KEY, JSON.stringify(next.map(({ source: { kind, path } }) => ({ kind, path }))));
    loaded = next;
    revision++;
  }

  function run<T>(operation: () => Promise<T> | T): Promise<T> {
    const result = queue.then(operation);
    queue = result.then(
      () => {},
      () => {},
    );
    return result;
  }

  async function ready() {
    if (!initialized) await reload();
  }

  return {
    list: () =>
      run(async () => {
        await ready();
        return loaded.map(({ source }) => source);
      }),
    refresh: () => run(reload),
    choose: (kind) =>
      run(async () => {
        const path = await files.pick(kind);
        if (!path) return;
        await ready();
        const item = await load({ kind, path });
        save([...loaded.filter(({ source }) => source.path !== path), item]);
      }),
    remove: (path) =>
      run(async () => {
        await ready();
        save(loaded.filter(({ source }) => source.path !== path));
      }),
    revision: () => revision,
    maps: (hash) =>
      run(async () => {
        await ready();
        // A CPU-specific database overrides an imported map for that CPU. Within a kind the last selection wins.
        const chosen: Partial<Record<Cpu, AddrMapJson>> = {};
        for (const kind of ['map', 'cf', 'sharc'] as const) {
          for (const item of loaded) {
            if (item.source.kind === kind) Object.assign(chosen, item.images.get(hash));
          }
        }
        return Object.values(chosen);
      }),
  };
}
