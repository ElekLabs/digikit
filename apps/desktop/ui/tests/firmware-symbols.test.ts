import { describe, expect, it } from 'vitest';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import type { SymbolKind } from '@digi/workbench-core/host/host.ts';
import { createFirmwareSymbols } from '../src/firmware-symbols';

const hash = 'a'.repeat(64);
function example(kind: SymbolKind, path: string) {
  const cpus = kind === 'map' ? ['cf', 'sharc'] : [kind];
  return [
    {
      device: 'example',
      firmware: '1.0',
      sha256: hash,
      json: JSON.stringify({
        kind: 'dev',
        key: { image_sha256: hash },
        entries: cpus.map((cpu) => ({ cpu, name: path, layer: 'fw', addr: 4096, size: 12 })),
      }),
    },
  ];
}
describe('Workbench firmware symbol sources', () => {
  it('resolves the recorded image and CPU, persists only paths, and restores the lower priority source on removal', async () => {
    const storage = memoryKeyValue();
    const files = {
      pick: async (kind: SymbolKind) => '/local/' + kind,
      read: async (kind: SymbolKind, path: string) => example(kind, path),
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
    const files = {
      pick: async () => (gone ? null : '/sample'),
      read: async (kind: SymbolKind) => {
        if (gone) throw new Error('file unavailable');
        return example(kind, 'name');
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
