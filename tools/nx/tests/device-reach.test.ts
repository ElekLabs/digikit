import { expect, it } from 'vitest';
import { resolve } from 'node:path';
import { parseOwners } from '../area-paths.mjs';
import { reachingDeviceTests } from '../device-reach.mjs';
import { areaSelection } from '../test-subset.mjs';

it('finds the files a task reads, in few calls, and drops the rest', () => {
  const calls: string[][] = [];
  const files = ['a', 'b', 'c', 'd', 'e', 'f'];
  const got = reachingDeviceTests(
    '.',
    files,
    (f: string[]) => (calls.push(f), f.some((x) => x === 'b' || x === 'e') ? ['p:test-device--x--half'] : ['p:pack']),
  );
  expect(got).toEqual(['b', 'e']);
  expect(calls.length).toBeLessThan(2 * files.length);
  // a task that is no test task does not count
  expect(reachingDeviceTests('.', ['a'], () => ['p:pack'])).toEqual([]);
});

it('stops at the first file a task reads when asked for one', () => {
  const calls: string[][] = [];
  const files = Array.from({ length: 64 }, (_, i) => `f${i}`);
  const all = (f: string[]) => (calls.push(f), ['p:test-device--x--half']);
  expect(reachingDeviceTests('.', files, all, { first: true })).toEqual(['f0']);
  // one bisection path down to a single file, never the whole tree
  expect(calls.length).toBeLessThanOrEqual(Math.log2(files.length) + 1);
});

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
    'image-modules/adapters/card-guard/src/cf/guard.c',
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
