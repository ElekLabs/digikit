// Header records for the simulated Transfer library. The device-side service
// dependency of a machine or effect makes the normal installer exercise that route.
import { RECORD_KIND, RESOURCE, TABLE_MAGIC } from '@digi/dk-session';
import { nameHash } from '@digi/workbench-core/modules/services.ts';
import { fakeDkm } from '@digi/workbench-core/target-ops/fake-store.ts';

export interface ModuleFixture {
  group: 'machine' | 'fx' | 'tool';
  keys: string[];
  uses: Record<string, number>;
}

export function previewModule(module: { id: string; version: string }, fixture: ModuleFixture, size: number) {
  const { keys, group } = fixture;
  const words = [TABLE_MAGIC, 1 << 16];
  // Additional services exercised by the preview's Transfer scenarios.
  const extra: Record<string, string[]> = { 'digikit/shot': ['samples'], 'digikit/track-manager': ['shortcuts'] };
  const services = [
    ...(group === 'machine' ? ['machines'] : group === 'fx' ? ['fx'] : []),
    ...(extra[module.id] ?? []),
  ];
  if (services.length) {
    words[1]++;
    words.push((1 << 16) | services.length, services.length * 12);
    for (const service of services) words.push(nameHash(service), 0, 1);
  }
  const uses = Object.entries(RESOURCE).flatMap(([name, id]) => {
    const count = fixture.uses[name.toLowerCase()];
    return count ? [[id, count]] : [];
  });
  if (uses.length) {
    words[1]++;
    words.push((2 << 16) | uses.length, 8 * uses.length, ...uses.flat());
  }
  if (keys.length) {
    const machine = group === 'machine';
    words[1]++;
    // KEYS: one group of module-owned record keys.
    words.push((3 << 16) | 1, 8 + 4 * keys.length, machine ? RECORD_KIND.MACHINE : RECORD_KIND.EFFECT, keys.length);
    words.push(...keys.map((key) => nameHash(`${module.id}/${key}`)));
  }
  const table = new Uint8Array(words.length * 4);
  const view = new DataView(table.buffer);
  words.forEach((word, i) => view.setUint32(4 * i, word));
  const version = module.version.split('.').reduce((n, part) => (n << 8) | Number(part), 0);
  return fakeDkm({ name: module.id, version, size, table, imageSize: 256 }).slice();
}
