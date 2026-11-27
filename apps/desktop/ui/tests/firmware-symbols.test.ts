import { describe, expect, it } from 'vitest';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import type { SymbolKind } from '@digi/workbench-core/host/host.ts';
import { createFirmwareSymbols } from '../src/firmware-symbols';

const hash = 'a'.repeat(64);
function example(kind: SymbolKind, path: string, sha256 = hash) {
  const cpus = kind === 'map' ? ['cf', 'sharc'] : [kind];
  return [
    {
      device: 'example',
      firmware: '1.0',
      sha256,
      json: JSON.stringify({
        kind: 'dev',
        key: { image_sha256: sha256 },
        entries: cpus.map((cpu) => ({ cpu, name: path, layer: 'fw', addr: 4096, size: 12 })),
      }),
    },
  ];
}
describe('Workbench firmware symbol sources', () => {
  it('resolves the recorded image and CPU, persists only paths, and restores the lower priority source on removal', async () => {
    const storage = memoryKeyValue();
    const files: Parameters<typeof createFirmwareSymbols>[1] = {
      pick: async (kind) => '/local/' + kind,
      read: async (kind, path) => example(kind === 'catalog' ? 'sharc' : kind, path),
    };
    const symbols = createFirmwareSymbols(storage, files);
    await Promise.all([symbols.choose('cf'), symbols.choose('map')]);
    const names = async () =>
      (await symbols.maps(hash)).flatMap((map) => (map.entries as { name: string }[]).map((e) => e.name));
    expect(await names()).toEqual(['/local/cf', '/local/map']);
    expect(await symbols.maps('b'.repeat(64))).toEqual([]);
    expect([...storage.data.values()].join('')).not.toContain('image_sha256');
    const restored = createFirmwareSymbols(storage, files);
    expect(await restored.maps(hash)).toEqual(await symbols.maps(hash));
    await symbols.remove('/local/cf');
    expect(await names()).toEqual(['/local/map', '/local/map']);
  });
  it('discards cached names when refresh fails and preserves the selection when the dialog is cancelled', async () => {
    let gone = false;
    const files: Parameters<typeof createFirmwareSymbols>[1] = {
      pick: async () => (gone ? null : '/sample'),
      read: async (kind) => {
        if (gone) throw new Error('file unavailable');
        return example(kind === 'catalog' ? 'sharc' : kind, 'name');
      },
    };
    const symbols = createFirmwareSymbols(memoryKeyValue(), files);
    await symbols.choose('sharc');
    expect(await symbols.maps(hash)).toHaveLength(1);
    gone = true;
    await symbols.choose('map');
    expect(await symbols.list()).toHaveLength(1);
    await symbols.refresh();
    expect(await symbols.maps(hash)).toEqual([]);
    expect((await symbols.list())[0]?.error).toBe('file unavailable');
  });
});

it('keeps a catalog link when a different source supplies the names for the same image and CPU', async () => {
  const review = {
    cpu: 'sharc' as const,
    path: '/catalog/review.html',
    url: 'file:///catalog/review.html',
    entries: [4096],
  };
  const symbols = createFirmwareSymbols(memoryKeyValue(), {
    pick: async (kind) => '/local/' + kind,
    read: async (kind, path) =>
      example(kind === 'catalog' ? 'sharc' : kind, path).map((map) => ({
        ...map,
        review: kind === 'map' ? review : undefined,
      })),
  });
  await symbols.choose('map');
  await symbols.choose('sharc');
  const [cf, sharc] = await symbols.maps(hash);
  expect(sharc?.entries).toMatchObject([{ name: '/local/sharc' }]);
  expect(sharc?.review).toEqual(review);
  expect(cf?.review).toBeUndefined();
  expect(await symbols.maps('b'.repeat(64))).toEqual([]);
  await symbols.remove('/local/map');
  expect((await symbols.maps(hash))[0]?.review).toBeUndefined();
});

describe('per-release firmware profiles', () => {
  const peerHash = 'b'.repeat(64);
  function release(device: string, sha256: string) {
    const root = '/checkout/out/firmware/' + device + '-1.0';
    return {
      device,
      firmware: '1.0',
      sha256,
      defaults: {
        cf: root + '/main/firmware.sqlite',
        sharc: root + '/dsp/firmware.sqlite',
        catalog: root + '/dsp/review.html',
      },
    };
  }
  const main = release('example', hash);
  const peer = release('peer', peerHash);

  function fixtures(storage = memoryKeyValue()) {
    let selected: string | null = '/custom/review.html';
    let legacyMissing = false;
    return {
      setPath: (path: string | null) => (selected = path),
      setLegacyMissing: (missing: boolean) => (legacyMissing = missing),
      symbols: createFirmwareSymbols(storage, {
        releases: async () => [main, peer],
        pick: async () => selected,
        read: async (kind, path) => {
          if (path === peer.defaults.cf || (legacyMissing && path.startsWith('/legacy/')))
            throw new Error('File is missing');
          const image = path.includes('/peer-1.0/') || path === '/custom/wrong.html' ? peer : main;
          const [exampleMap] = example(kind === 'catalog' ? 'sharc' : kind, path, image.sha256);
          const review =
            kind === 'catalog' ? { cpu: 'sharc' as const, path, url: 'file://' + path, entries: [4096] } : undefined;
          return [{ ...exampleMap, ...image, review }];
        },
      }),
    };
  }

  it('discovers defaults by hash and applies a verified catalog override to only its release', async () => {
    const storage = memoryKeyValue();
    const { symbols, setPath } = fixtures(storage);
    const [first, second] = (await symbols.profiles?.()) ?? [];
    expect(first?.sources.catalog).toEqual({ path: main.defaults.catalog, override: false });
    expect(second?.sources.cf.error).toBe('File is missing');
    expect(storage.get('digikit.firmwareProfiles.v1')).toBeNull();
    const linkedCatalog = async (sha256: string) =>
      (await symbols.maps(sha256)).flatMap((map) => (map.review ? [map.review] : []));
    expect(await linkedCatalog(hash)).toMatchObject([{ path: main.defaults.catalog }]);

    setPath('/custom/wrong.html');
    await expect(symbols.chooseProfile?.(hash, 'catalog')).rejects.toThrow('does not match');
    expect(await linkedCatalog(hash)).toMatchObject([{ path: main.defaults.catalog }]);

    setPath('/custom/review.html');
    await symbols.chooseProfile?.(hash, 'catalog');
    expect(await linkedCatalog(hash)).toMatchObject([{ path: '/custom/review.html' }]);
    expect(await linkedCatalog(peerHash)).toMatchObject([{ path: peer.defaults.catalog }]);

    const restarted = fixtures(storage).symbols;
    expect((await restarted.profiles?.())?.[0]?.sources.catalog).toEqual({
      path: '/custom/review.html',
      override: true,
    });
    await restarted.resetProfile?.(hash, 'catalog');
    expect((await restarted.profiles?.())?.[0]?.sources.catalog.path).toBe(main.defaults.catalog);
    expect(storage.get('digikit.firmwareProfiles.v1')).not.toContain('/custom/review.html');
  });

  it('migrates applicable legacy selections once and leaves the default selected after a reset', async () => {
    const storage = memoryKeyValue({
      'digikit.firmwareSymbols.v1': JSON.stringify([{ kind: 'sharc', path: '/legacy/firmware.sqlite' }]),
    });
    const { symbols, setLegacyMissing } = fixtures(storage);
    setLegacyMissing(true);
    expect((await symbols.profiles?.())?.[0]?.sources.sharc.path).toBe(main.defaults.sharc);
    setLegacyMissing(false);
    await symbols.refresh();
    expect((await symbols.profiles?.())?.[0]?.sources.sharc).toEqual({
      path: '/legacy/firmware.sqlite',
      override: true,
    });
    await symbols.resetProfile?.(hash, 'sharc');
    const restarted = fixtures(storage).symbols;
    expect((await restarted.profiles?.())?.[0]?.sources.sharc).toEqual({ path: main.defaults.sharc, override: false });
  });
});
