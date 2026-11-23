// The system package on every test card: installed at the store path stage 1 reads, and part of the card key, so a changed package gives new cards (and new boot snapshots).
import fs from 'node:fs';
import path from 'node:path';
import { afterAll, expect, test, vi } from 'vitest';

// hoisted: the mock below (and the modules that call `repoRoot` when they load) need the directory first
const dir = vi.hoisted(() => {
  const nodeFs = require('node:fs') as typeof import('node:fs');
  const nodeOs = require('node:os') as typeof import('node:os');
  const nodePath = require('node:path') as typeof import('node:path');
  return nodeFs.mkdtempSync(nodePath.join(nodeOs.tmpdir(), 'syspkg-'));
});

vi.mock('@digi/test-support', async (original) => ({
  ...(await original<object>()),
  repoRoot: () => dir,
}));
import { cardKey } from '../runtime/card-cache.ts';
import { installSystemPackage, SYSTEM_PACKAGE_STORE_PATH, systemPackageFile } from '../runtime/system-package.ts';

// what `toolIdentity` reads: the profile trees and the plusdrive binary
for (const tree of ['devices', 'ports']) fs.mkdirSync(path.join(dir, tree), { recursive: true });
fs.mkdirSync(path.join(dir, 'dist/plusdrive'), { recursive: true });
fs.writeFileSync(path.join(dir, 'dist/plusdrive/plusdrive'), 'plusdrive');
afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

const writePackage = (bytes: string) => {
  fs.mkdirSync(path.dirname(systemPackageFile()), { recursive: true });
  fs.writeFileSync(systemPackageFile(), bytes);
};

test('a missing package is an error that names the Nx target, and no card key stands for it', () => {
  fs.rmSync(path.dirname(systemPackageFile()), { recursive: true, force: true });
  expect(() => installSystemPackage(path.join(dir, 'seed'))).toThrow('nx run core/runtime:package');
  expect(cardKey('dn2', { a: 1 })).toMatch(/^[0-9a-f]{32}$/);
});

test('the package goes to its store path below the card folder digikit/', () => {
  writePackage('package one');
  const seed = path.join(dir, 'seed-one');
  installSystemPackage(seed);
  expect(fs.readFileSync(path.join(seed, 'digikit', SYSTEM_PACKAGE_STORE_PATH), 'utf8')).toBe('package one');
});

test('the card key follows the package bytes and nothing else about the file', () => {
  writePackage('package one');
  const one = cardKey('dn2', { a: 1 });
  expect(cardKey('dn2', { a: 1 }), 'the same package gives the same key').toBe(one);
  writePackage('package two');
  expect(cardKey('dn2', { a: 1 }), 'a changed package gives a new key').not.toBe(one);
  writePackage('package one');
  expect(cardKey('dn2', { a: 1 }), 'and the old bytes the old key').toBe(one);
});
