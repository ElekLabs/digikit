// The site's Transfer library (src/transfer/web-library.ts): the shipped index, the files it names, the files the user adds, the source files.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'vitest';
import { createWebLibrary } from '../src/transfer/web-library.ts';

const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');
/** A .dkm header: magic, a name at 0x40 and a version at 0x34; the rest zero. */
function dkm(name: string): Uint8Array {
  const b = new Uint8Array(0xa0);
  b.set([0x44, 0x4b, 0x4d, 0x46]);
  b.set(new TextEncoder().encode(name), 0x40);
  return b;
}
const PLUCK = dkm('digikit/pluck');
const index = {
  modules: [
    {
      id: 'digikit/pluck',
      name: 'PLUCK',
      summary: 's',
      kind: 'machines',
      version: '0.1.0',
      devices: { dt2: { file: 'dt2/pluck.dkm', size: PLUCK.length, sha256: sha(PLUCK) } },
      source: ['module.toml', 'src/module.c'],
    },
  ],
};

function setup(overrides: Record<string, Uint8Array> = {}) {
  const asked: string[] = [];
  const lib = createWebLibrary({
    bytes: async (p) => {
      asked.push(p);
      const b = overrides[p] ?? (p === 'dt2/pluck.dkm' ? PLUCK : undefined);
      if (!b) throw new Error(`${p}: 404`);
      return b;
    },
    text: async (p) => (p === 'index.json' ? JSON.stringify(index) : `text of ${p}`),
  });
  return { ...lib, asked };
}

test('the library lists the shipped files of the device and reads them', async () => {
  const { library } = setup();
  assert.deepEqual(await library.scan('dt2'), [{ path: 'dt2/pluck.dkm', size: PLUCK.length }]);
  assert.deepEqual(await library.scan('dn2'), []);
  assert.deepEqual(await library.read('dt2', 'dt2/pluck.dkm'), PLUCK);
});

test('a shipped file that does not match the index is refused', async () => {
  const { library } = setup({ 'dt2/pluck.dkm': dkm('digikit/other') });
  await library.scan('dt2');
  await assert.rejects(library.read('dt2', 'dt2/pluck.dkm'), /does not match/);
});

test('added files join the list; one with the same module id replaces the shipped file; a non-module is refused', async () => {
  const { library } = setup();
  const lines = await library.addFiles!([{ name: 'mine.dkm', data: dkm('digikit/pluck') }]);
  assert.match(lines[0], /Added mine\.dkm \(digikit\/pluck/);
  const files = await library.scan('dt2');
  assert.deepEqual(
    files.map((f) => f.path),
    ['added/mine.dkm'],
  );
  assert.deepEqual(await library.read('dt2', 'added/mine.dkm'), dkm('digikit/pluck'));
  await assert.rejects(library.addFiles!([{ name: 'x.dkm', data: new Uint8Array(10) }]), /not a \.dkm/);
});

test('the sources are known after a scan and read from the site', async () => {
  const { library, source } = setup();
  assert.deepEqual(source.files('digikit/pluck'), []);
  await library.scan('dt2');
  assert.deepEqual(source.files('digikit/pluck'), ['module.toml', 'src/module.c']);
  assert.equal(await source.read('digikit/pluck', 'src/module.c'), 'text of src/digikit/pluck/src/module.c');
});
