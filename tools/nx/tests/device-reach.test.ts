import { expect, it } from 'vitest';
import { resolve } from 'node:path';
import { parseOwners } from '../area-paths.mjs';
import { reachingDeviceTests } from '../device-reach.mjs';
import { areaSelection } from '../test-subset.mjs';

// The real Nx graph of this checkout: the paths that reach device tests through their keys, not through a list of ours.
const root = resolve(import.meta.dirname, '../../..');
const ctx = {
  owners: parseOwners('shared = []'),
  serviceTags: new Set<string>(),
  recordTags: new Set<string>(),
  deviceOf: () => undefined,
  crateOf: () => undefined,
};
const wideOf = (file: string) =>
  areaSelection([], [], { changed: [file], ctx, reaches: (f: string[]) => reachingDeviceTests(root, f) });

it('goes wide for the inputs the keys hold that no hand list named, and not for docs', { timeout: 300_000 }, () => {
  const wide = [
    'runtime/platform/card-guard/src/cf/guard.c',
    'config/emulator.json',
    'devices/cycle-factors.json',
    'ports/bindings/include/dk_fw_draw.h',
  ];
  for (const file of wide) {
    const got = wideOf(file);
    expect(got.wideAll, file).toBe(true);
    expect(got.wide.map((w: { file: string }) => w.file)).toEqual([file]);
  }
  const docs = wideOf('docs/TESTING.md');
  expect(docs.wideAll).toBe(false);
  expect(docs.tests).toEqual([]);
});
