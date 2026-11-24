// Keep user-selected paths in preferences. Firmware-derived names live in this app instance only.
import type {
  FirmwareProfile,
  ProfileSymbolKind,
  SymbolImageMap,
  SymbolKind,
  SymbolRelease,
  SymbolReview,
  SymbolSource,
  SymbolsHost,
} from '@digi/workbench-core/host/host.ts';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import type { AddrMapJson, Cpu } from '@digi/dk-session/dkprof';

interface SymbolFiles {
  pick(kind: SymbolKind | 'catalog'): Promise<string | null>;
  read(kind: SymbolKind | 'catalog', path: string): Promise<SymbolImageMap[]>;
  releases?(): Promise<SymbolRelease[]>;
  catalogFrame?: SymbolsHost['catalogFrame'];
  openReview?: SymbolsHost['openReview'];
}

type Selection = Pick<SymbolSource, 'kind' | 'path'>;
type Channels = Partial<Record<Cpu, AddrMapJson>>;
type ProfileOverrides = Record<string, Partial<Record<ProfileSymbolKind, string | null>>>;
type ProfileData = { profile: FirmwareProfile; maps: Channels };

const kinds = ['cf', 'sharc', 'catalog'] as const;
const cpus = ['cf', 'sharc'] as const;
interface Loaded {
  source: SymbolSource;
  images: Map<string, Channels>;
  reviews: Map<string, SymbolReview>;
}

const KEY = 'digikit.firmwareSymbols.v1';
const PROFILES = 'digikit.firmwareProfiles.v1';

function profileOverrides(storage: KeyValue): ProfileOverrides | undefined {
  try {
    const saved: unknown = JSON.parse(storage.get(PROFILES) ?? 'null');
    if (!saved || typeof saved !== 'object' || Array.isArray(saved)) return undefined;
    const overrides: ProfileOverrides = {};
    for (const [hash, fields] of Object.entries(saved)) {
      if (!/^[a-f0-9]{64}$/.test(hash) || !fields || typeof fields !== 'object') continue;
      const paths: ProfileOverrides[string] = {};
      for (const kind of kinds) {
        const value = fields[kind];
        if (value === null || (typeof value === 'string' && value)) paths[kind] = value;
      }
      overrides[hash] = paths;
    }
    return overrides;
  } catch {
    return undefined;
  }
}

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
  let releases: SymbolRelease[] = [];
  let overrides: ProfileOverrides = profileOverrides(storage) ?? {};
  const profiles = new Map<string, ProfileData>();
  let revision = 0;
  let queue = Promise.resolve();

  async function load(kind: SymbolKind | 'catalog', path: string): Promise<Loaded> {
    const source: SymbolSource = { kind: kind === 'catalog' ? 'map' : kind, path, images: [] };
    const images: Loaded['images'] = new Map();
    const reviews: Loaded['reviews'] = new Map();
    try {
      for (const { json, review, ...identity } of await files.read(kind, path)) {
        const map = JSON.parse(json);
        const channels: Channels = {};
        for (const cpu of cpus) {
          const entries = map.entries.filter((entry: { cpu: Cpu }) => entry.cpu === cpu);
          if (entries.length) channels[cpu] = { ...map, entries, review: review?.cpu === cpu ? review : undefined };
        }
        images.set(identity.sha256, channels);
        if (review) reviews.set(identity.sha256, review);
        source.images.push(identity);
      }
    } catch (error) {
      source.error = error instanceof Error ? error.message : String(error);
      source.images = [];
      images.clear();
      reviews.clear();
    }
    return { source, images, reviews };
  }

  async function reload() {
    const next: Loaded[] = [];
    for (const { kind, path } of selections(storage)) next.push(await load(kind, path));
    releases = (await files.releases?.()) ?? [];
    loaded = next;
    profiles.clear();
    migrateLegacySources();
    initialized = true;
    revision++;
  }

  function migrateLegacySources() {
    const migrated = { ...overrides };
    let changed = false;
    for (const release of releases) {
      const paths = { ...migrated[release.sha256] };
      const latestReview = loaded.findLast((item) => item.reviews.has(release.sha256));
      const candidates = {
        cf: loaded.findLast((item) => item.source.kind === 'cf' && item.images.get(release.sha256)?.cf)?.source.path,
        sharc: loaded.findLast((item) => item.source.kind === 'sharc' && item.images.get(release.sha256)?.sharc)?.source
          .path,
        catalog: latestReview?.reviews.get(release.sha256)?.path,
      };
      for (const kind of kinds) {
        const path = candidates[kind];
        if (paths[kind] !== undefined || !path) continue;
        paths[kind] = path === release.defaults[kind] ? null : path;
        changed = true;
      }
      if (Object.keys(paths).length) migrated[release.sha256] = paths;
    }
    if (changed) {
      overrides = migrated;
      storage.set(PROFILES, JSON.stringify(migrated));
    }
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

  function releaseFor(hash: string) {
    return releases.find((release) => release.sha256 === hash);
  }

  async function profileFor(release: SymbolRelease): Promise<ProfileData> {
    const { defaults, ...image } = release;
    const { sha256 } = image;
    const cached = profiles.get(sha256);
    if (cached) return cached;
    const paths = overrides[sha256];
    const path = (kind: ProfileSymbolKind) => paths?.[kind] ?? defaults[kind];
    const [cf, sharc, catalog] = await Promise.all(kinds.map((kind) => load(kind, path(kind))));
    const review = catalog.reviews.get(sha256);
    const row = (kind: ProfileSymbolKind, item: Loaded, found: unknown) => ({
      path: path(kind),
      override: !!paths?.[kind],
      error: item.source.error ?? (found ? undefined : 'The file does not match this firmware release.'),
    });
    const cfMap = cf.images.get(sha256)?.cf;
    const sharcMap = sharc.images.get(sha256)?.sharc;
    const catalogMap = catalog.images.get(sha256)?.sharc;
    const maps: Channels = {};
    if (cfMap) maps.cf = cfMap;
    const dsp = sharcMap ?? catalogMap;
    if (dsp) maps.sharc = { ...dsp, review };
    const value = {
      profile: {
        ...image,
        sources: {
          cf: row('cf', cf, cfMap),
          sharc: row('sharc', sharc, sharcMap),
          catalog: row('catalog', catalog, review),
        },
        review,
      },
      maps,
    };
    profiles.set(sha256, value);
    return value;
  }

  function setOverride(sha256: string, kind: ProfileSymbolKind, path?: string) {
    const next = { ...overrides };
    const slots = { ...next[sha256] };
    // Remember an explicit reset so a legacy registration does not become an override again.
    slots[kind] = path ?? null;
    next[sha256] = slots;
    storage.set(PROFILES, JSON.stringify(next));
    overrides = next;
    profiles.delete(sha256);
    revision++;
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
        const item = await load(kind, path);
        save([...loaded.filter(({ source }) => source.path !== path), item]);
      }),
    remove: (path) =>
      run(async () => {
        await ready();
        save(loaded.filter(({ source }) => source.path !== path));
      }),
    profiles: files.releases
      ? () =>
          run(async () => {
            await ready();
            return (await Promise.all(releases.map(profileFor))).map(({ profile }) => profile);
          })
      : undefined,
    chooseProfile: (sha256, kind) =>
      run(async () => {
        await ready();
        if (!releaseFor(sha256)) throw new Error('Unknown firmware release.');
        const path = await files.pick(kind);
        if (!path) return;
        const item = await load(kind, path);
        const match = kind === 'catalog' ? item.reviews.get(sha256) : item.images.get(sha256)?.[kind];
        if (!match) throw new Error(item.source.error ?? 'The selected file does not match this firmware release.');
        setOverride(sha256, kind, path);
      }),
    resetProfile: (sha256, kind) =>
      run(async () => {
        await ready();
        setOverride(sha256, kind);
      }),
    revision: () => revision,
    catalogFrame: files.catalogFrame,
    openReview: files.openReview,
    maps: (hash) =>
      run(async () => {
        await ready();
        const release = releaseFor(hash);
        const chosen: Channels = {};
        for (const kind of ['map', 'cf', 'sharc'] as const) {
          // For a managed release the profile owns database choices. Existing developer maps can fill gaps.
          if (release && kind !== 'map') continue;
          for (const item of loaded) {
            if (item.source.kind !== kind) continue;
            const channels = item.images.get(hash);
            for (const cpu of cpus) {
              const map = channels?.[cpu];
              if (map) chosen[cpu] = { ...map, review: map.review ?? chosen[cpu]?.review };
            }
          }
        }
        if (release) {
          const data = await profileFor(release);
          Object.assign(chosen, data.maps);
          if (chosen.sharc) chosen.sharc = { ...chosen.sharc, review: data.profile.review };
        }
        return Object.values(chosen);
      }),
  };
}
