import { mkdtempSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import {
  assertTrackedObjectsUnchanged,
  compareManifests,
  pathMapper,
  strictObjectHash,
  trackedObjectHashes,
} from '../byte-manifest.mjs';

const shape = (source: string, flags = '["-O1"]') =>
  Buffer.from(`{\n "source_sha256": "${source.repeat(64)}",\n "toolchain": {"cflags": ${flags}}\n}\n`);

it('masks only the source hash value, keeping flags, spacing and all other bytes', () => {
  const original = strictObjectHash(shape('a'), 'example');
  const moved = strictObjectHash(shape('b'), 'example');
  expect(moved.source).not.toBe(original.source);
  expect(moved.sha256).toBe(original.sha256);
  expect(strictObjectHash(shape('a', '["-O2"]'), 'example').sha256).not.toBe(original.sha256);
  expect(strictObjectHash(shape('a', '[ "-O1" ]'), 'example').sha256).not.toBe(original.sha256);
  expect(() => strictObjectHash(shape('z'), 'example')).toThrow('invalid source_sha256');
});

it('fails if preparation refreshes tracked objects, even for a source hash alone', () => {
  const root = mkdtempSync(join(tmpdir(), 'byte-manifest-test-'));
  const paths = ['other.dko.json', 'source.dko.json', 'removed.dko.json', 'flags.dko.json'];
  try {
    for (const path of paths) writeFileSync(join(root, path), shape('a'));
    const before = trackedObjectHashes(root, paths);
    expect(() => assertTrackedObjectsUnchanged(before, trackedObjectHashes(root, paths))).not.toThrow();
    writeFileSync(join(root, 'source.dko.json'), shape('b'));
    writeFileSync(join(root, 'flags.dko.json'), shape('a', '["-O2"]'));
    unlinkSync(join(root, 'removed.dko.json'));
    expect(() => assertTrackedObjectsUnchanged(before, trackedObjectHashes(root, paths))).toThrow(
      'Preparation refreshed tracked objects. Review these changes before rerunning the byte manifest:\n' +
        'flags.dko.json\nremoved.dko.json\nsource.dko.json',
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

const baseline = () => ({
  version: 1,
  device: 'example',
  entries: [
    { kind: 'image', path: 'example-1.2/device' },
    { kind: 'plan', path: 'example-1.2/device' },
    { kind: 'addrmap', path: 'example-1.2/device' },
    { kind: 'pack', path: 'dist/modules/example/test.dkm' },
    { kind: 'verdicts', path: 'dist/bytes/pack-verdicts.json' },
    { kind: 'generated', path: 'modules/core/build/gen/foo.h' },
    { kind: 'object', path: 'modules/core/build/foo.dko.json', builtSha256: 'b'.repeat(64) },
    { kind: 'core-object-list', path: 'modules/core/build/foo.dko.json' },
  ].map((entry) => ({ ...entry, sha256: 'a'.repeat(64) })),
});

it('maps moved generated files, tracked objects and object-list path keys for comparison', () => {
  const before = baseline();
  const after = baseline();
  for (const entry of after.entries) entry.path = entry.path.replace('modules/core/', 'runtime/core/');
  expect(compareManifests(before, after, 'example', { 'modules/core': 'runtime/core' })).toEqual([]);
  expect(compareManifests(before, after, 'example')).toContain('extra object runtime/core/build/foo.dko.json');
  const rename = pathMapper({ 'modules/core': 'runtime/core', 'modules/core/build/gen': 'sdk/gen' });
  expect(rename('modules/core/build/gen/foo.h')).toBe('sdk/gen/foo.h');
  expect(rename('modules/core-extra/build/foo.dko.json')).toBe('modules/core-extra/build/foo.dko.json');
});

it('reports changed, missing and extra artifacts and distinguishes a fresh build mismatch', () => {
  const before = baseline();
  const after = baseline();
  after.entries.find((entry) => entry.kind === 'object')!.builtSha256 = 'c'.repeat(64);
  after.entries.find((entry) => entry.kind === 'pack')!.sha256 = 'b'.repeat(64);
  after.entries.push({ kind: 'generated', path: 'sdk/extra.h', sha256: 'c'.repeat(64) });
  after.entries.find((entry) => entry.kind === 'generated')!.path = 'sdk/another.h';
  expect(compareManifests(before, after, 'example')).toEqual([
    'missing generated modules/core/build/gen/foo.h',
    'extra generated sdk/another.h',
    'extra generated sdk/extra.h',
    'changed build of object modules/core/build/foo.dko.json',
    'changed pack dist/modules/example/test.dkm',
  ]);
});

it('rejects incomplete manifests and move maps that collide with an unmoved entry', () => {
  const before = baseline();
  const current = baseline();
  current.entries = current.entries.filter((entry) => entry.kind !== 'plan');
  expect(() => compareManifests(before, current, 'example')).toThrow('missing plan entries');
  before.entries.push({ kind: 'generated', path: 'sdk/gen/foo.h', sha256: 'a'.repeat(64) });
  expect(() => compareManifests(before, baseline(), 'example', { 'modules/core/build/gen': 'sdk/gen' })).toThrow(
    'duplicate generated sdk/gen/foo.h',
  );
  expect(() => pathMapper({ 'modules/core': 'runtime/core', 'modules/other': 'runtime/core' })).toThrow(
    'duplicate runtime/core',
  );
  expect(() => pathMapper({ 'modules/core': '../runtime/core' })).toThrow('Invalid repository-relative path');
});
