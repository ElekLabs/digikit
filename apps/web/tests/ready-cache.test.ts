// The ready-state cache logic with an in-memory store, on the DN2 ready inputs when present (and on a small synthetic state always): the round trip
// is exact, key and version handling work, and a corrupt entry is dropped.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, test } from 'vitest';
import { memoryBlobStore } from '@digi/workbench-core/blob-store.ts';
import { ReadyCache, sha256Hex, type ReadyState } from '../src/ready-cache.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

/** The DN2 ready state of the ready inputs, when they are there. */
function readyInputsState(): ReadyState | undefined {
  try {
    const ready = path.resolve(
      root,
      process.env.DIGI_EMU_READY_DIR ??
        JSON.parse(fs.readFileSync(path.join(root, 'tools/emulator/ready_inputs.json'), 'utf8')).ready_dir,
    );
    if (!fs.existsSync(path.join(ready, 'digi-audio-m5.snap'))) return undefined;
    const read = (n: string) => new Uint8Array(fs.readFileSync(path.join(ready, n)));
    return {
      image: read('digi-audio-dn2-image.bin'),
      dsp: read('digi-audio-m5.snap.dsp'),
      snapshot: read('digi-audio-m5.snap'),
    };
  } catch {
    return undefined;
  }
}

const synthetic: ReadyState = {
  image: new Uint8Array([1, 2, 3]),
  dsp: new Uint8Array(1000).fill(7),
  snapshot: new Uint8Array(5000),
};
const real = readyInputsState();
const syx = new Uint8Array([9, 9, 9]);

describe.each([
  ['a synthetic state', synthetic],
  ...(real ? ([['the DN2 ready inputs', real]] as [string, ReadyState][]) : []),
])('ready cache on %s', (_name, state) => {
  test('the round trip is exact; an unknown key is a miss', async () => {
    const cache = new ReadyCache(memoryBlobStore());
    const key = await sha256Hex(syx);
    assert.equal(key.length, 64);
    assert.equal(await cache.load(key), undefined);
    await cache.save(key, state);
    const back = await cache.load(key);
    assert.ok(back);
    for (const k of ['image', 'dsp', 'snapshot'] as const)
      assert.ok(Buffer.from(back[k]).equals(Buffer.from(state[k])), k);
    assert.equal(await cache.load('0'.repeat(64)), undefined);
  });

  test('a corrupt entry is dropped, not returned', async () => {
    const store = memoryBlobStore();
    const cache = new ReadyCache(store);
    const key = await sha256Hex(syx);
    await cache.save(key, state);
    await store.put(ReadyCache.key(key), new Uint8Array([1, 2, 3, 4]));
    assert.equal(await cache.load(key), undefined);
    assert.equal(await store.get(ReadyCache.key(key)), undefined);
  });
});
