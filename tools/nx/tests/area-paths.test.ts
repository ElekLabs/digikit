import { expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { deviceShorts, filesOf } from '../device-files.mjs';
import {
  areaContext,
  areasOfPath,
  crateOf,
  externalLockChanges,
  modulePathOf,
  ownerOf,
  parseOwners,
  OWNERS_FILE,
} from '../area-paths.mjs';

const owners = parseOwners(`
shared = ["core", "dkrt::lib", "dkrt::entry", "dkrt_abi"]
"C:boot" = ["dkrt::boot", "dkrt::safe_mode"]
"C:loading" = ["dkrt::loader", "dkrt_lifecycle"]
"S:lfo" = ["dkrt_svc_lfo"]
`);
const crates: Record<string, { name: string; dir: string }> = {
  'runtime/core/src/rs/': { name: 'dkrt', dir: 'runtime/core/src/rs' },
  'runtime/core/lifecycle/': { name: 'dkrt-lifecycle', dir: 'runtime/core/lifecycle' },
  'services/lfo/': { name: 'dkrt-svc-lfo', dir: 'services/lfo' },
  'services/machines/': { name: 'dkrt-svc-machines', dir: 'services/machines' },
};
const ctx = {
  owners,
  serviceTags: new Set(['S:machines', 'S:lfo']),
  recordTags: new Set(['S:machines', 'S:lfo', 'A:core/rows']),
  portUsers: (file: string) =>
    ({
      'ports/fw/lfo.toml': ['core/services/lfo'],
      'ports/fw/param.toml': ['core/rows', 'core/services/machines'],
      'ports/fw/machine.toml': ['core/services/machines', 'core/runtime'],
    })[file] ?? [],
  deviceOf: (file: string) => (file.startsWith('devices/x-device') ? 'xd' : undefined),
  crateOf: (file: string) => Object.entries(crates).find(([prefix]) => file.startsWith(prefix))?.[1],
};

it('maps a module path to the longest owner prefix at a :: boundary', () => {
  expect(ownerOf('dkrt::boot', owners)).toBe('C:boot');
  expect(ownerOf('dkrt::loader::scan', owners)).toBe('C:loading');
  expect(ownerOf('dkrt::bootstrap', owners)).toBeUndefined();
  expect(ownerOf('dkrt::lib', owners)).toBe('shared');
  expect(ownerOf('dkrt_lifecycle::graph', owners)).toBe('C:loading');
});

it('derives the module path of a file from its crate and its place below src', () => {
  expect(modulePathOf('a/src/loader/scan.rs', 'dkrt', 'a')).toBe('dkrt::loader::scan');
  expect(modulePathOf('a/src/loader/mod.rs', 'dkrt', 'a')).toBe('dkrt::loader');
  expect(modulePathOf('a/src/lib.rs', 'dkrt-lifecycle', 'a')).toBe('dkrt_lifecycle::lib');
  expect(modulePathOf('a/build.rs', 'dkrt', 'a')).toBeUndefined();
});

it('finds the nearest Cargo package, skipping a workspace manifest', () => {
  const files: Record<string, string> = {
    'w/Cargo.toml': '[workspace]\nmembers = ["c"]',
    'w/c/Cargo.toml': '[package]\nname = "dkrt-x"',
  };
  expect(crateOf('w/c/src/a/b.rs', (p: string) => files[p])).toEqual({ name: 'dkrt-x', dir: 'w/c' });
  expect(crateOf('w/other/a.rs', (p: string) => files[p])).toBeUndefined();
});

it('gives a runtime source file the area of its owner, a shared module none, a service its tag', () => {
  expect(areasOfPath('runtime/core/src/rs/src/boot.rs', ctx)).toEqual({ areas: ['C:boot'] });
  expect(areasOfPath('runtime/core/src/rs/src/loader/scan.rs', ctx)).toEqual({ areas: ['C:loading'] });
  expect(areasOfPath('runtime/core/lifecycle/src/graph.rs', ctx)).toEqual({ areas: ['C:loading'] });
  expect(areasOfPath('services/lfo/src/rows.rs', ctx)).toEqual({ areas: ['S:lfo'] });
  // no owner prefix: the service directory rule names the service
  expect(areasOfPath('services/machines/src/rows.rs', ctx)).toEqual({ areas: ['S:machines'] });
  expect(areasOfPath('runtime/core/src/rs/src/entry.rs', ctx)).toEqual({
    unknown: 'shared runtime module dkrt::entry',
  });
});

it('gives a test source none, and every other path without an area unknown', () => {
  expect(areasOfPath('runtime/core/src/rs/src/boot/tests.rs', ctx)).toEqual({ none: true });
  expect(areasOfPath('runtime/core/src/rs/src/fake_platform.rs', ctx)).toEqual({ none: true });
  for (const file of [
    'runtime/core/abi.toml',
    'sdk/include/dk.h',
    'services/fx/src/lib.rs', // a service without records
    'packages/patchkit/src/main.rs',
    'runtime/core/owners.toml',
    'ports/bindings/x.rs',
    'runtime/core/src/rs/build.rs', // a file outside src
    'runtime/core/src/rs/src/newmod.rs', // no owner prefix
    'devices/cycle-factors.json',
  ])
    expect(areasOfPath(file, ctx), file).toHaveProperty('unknown');
  expect(areasOfPath('runtime/core/src/rs/src/newmod.rs', ctx)).toEqual({
    unknown: 'dkrt::newmod has no owner in runtime/core/owners.toml',
  });
});

it('maps the port file of one plug-in point to the areas of its users, or to unknown when a user has none', () => {
  expect(areasOfPath('ports/fw/lfo.toml', ctx)).toEqual({ areas: ['S:lfo'] });
  expect(areasOfPath('ports/fw/param.toml', ctx)).toEqual({ areas: ['A:core/rows', 'S:machines'] });
  expect(areasOfPath('ports/fw/machine.toml', ctx)).toMatchObject({ unknown: expect.stringContaining('core/runtime') });
  expect(areasOfPath('ports/fw/other.toml', ctx)).toHaveProperty('unknown');
});

it('maps a device file to its device', () => {
  expect(areasOfPath('devices/x-device.toml', ctx)).toEqual({ devices: ['xd'] });
  expect(areasOfPath('devices/x-device-leds.toml', ctx)).toEqual({ devices: ['xd'] });
  expect(areasOfPath('devices/other.toml', ctx)).toHaveProperty('unknown');
});

it('maps the real runtime files by the real owners.toml and service records', () => {
  const root = resolve(import.meta.dirname, '../../..');
  const real = areaContext(root, [{ metadata: { runtimeAcceptance: [{ tags: ['S:machines', 'S:lfo'] }] } }]);
  expect(real.owners.length).toBeGreaterThan(20);
  expect(readFileSync(resolve(root, OWNERS_FILE), 'utf8')).toContain('"C:boot"');
  expect(areasOfPath('runtime/core/src/rs/src/boot.rs', real)).toEqual({ areas: ['C:boot'] });
  expect(areasOfPath('runtime/core/src/rs/src/package/loader/mod.rs', real)).toEqual({ areas: ['C:loading'] });
  expect(areasOfPath('runtime/core/src/rs/src/entry.rs', real)).toHaveProperty('unknown');
  expect(areasOfPath('ports/fw/lfo.toml', real)).toEqual({ areas: ['S:lfo'] });
  for (const short of deviceShorts(root)) {
    const [own, , ports] = filesOf(short, root);
    expect(areasOfPath(own!, real)).toEqual({ devices: [short] });
    expect(areasOfPath(ports!.replace('*', 'x'), real)).toEqual({ devices: [short] });
  }
  expect(areasOfPath('services/machines/src/lib.rs', real)).toEqual({ areas: ['S:machines'] });
});

it('gives a platform leaf the area of its own record tag, and a leaf without a tag none', () => {
  expect(areasOfPath('runtime/platform/rows/module.toml', ctx)).toEqual({ areas: ['A:core/rows'] });
  expect(areasOfPath('runtime/platform/rows/src/cf/type-dispatch.s', ctx)).toEqual({ areas: ['A:core/rows'] });
  expect(areasOfPath('runtime/platform/dsp-link/module.toml', ctx)).toHaveProperty('unknown');
});

const lock = (...packages: string[]) => packages.map((p) => `[[package]]\n${p}\n`).join('\n');
const own = (deps: string[] = []) =>
  `name = "dkrt-svc-x"\nversion = "0.1.0"\ndependencies = [${deps.map((d) => `"${d}"`).join(', ')}]`;
const serde = (version: string, checksum: string) =>
  `name = "serde"\nversion = "${version}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${checksum}"`;

it('ignores a lockfile change that touches only crates of the repository', () => {
  const before = lock(own(), serde('1.0.1', 'aa'));
  const after = lock(own(['dkrt-test-support']), serde('1.0.1', 'aa'), 'name = "dkrt-test-support"\nversion = "0.1.0"');
  expect(externalLockChanges(before, after)).toEqual([]);
  expect(areasOfPath('Cargo.lock', { ...ctx, lockChange: () => externalLockChanges(before, after) })).toEqual({
    none: true,
  });
});

it('names the registry or git packages a lockfile adds, removes or changes', () => {
  const before = lock(own(), serde('1.0.1', 'aa'));
  expect(externalLockChanges(before, lock(own(), serde('1.0.2', 'bb')))).toEqual([
    'serde 1.0.1 removed',
    'serde 1.0.2 added',
  ]);
  expect(externalLockChanges(before, lock(own(), serde('1.0.1', 'cc')))).toEqual(['serde 1.0.1 changed']);
  expect(externalLockChanges(undefined, before)).toEqual(['serde 1.0.1 added']);
  expect(externalLockChanges(before, undefined)).toEqual(['serde 1.0.1 removed']);
  const found = areasOfPath('services/lfo/dks/Cargo.lock', {
    ...ctx,
    lockChange: () => externalLockChanges(before, lock(own(), serde('1.0.1', 'cc'))),
  });
  expect(found).toEqual({ unknown: 'services/lfo/dks/Cargo.lock changes external package(s): serde 1.0.1 changed' });
});

it('maps svc-rt to C:loading and a dev-only crate to nothing, in the real checkout', () => {
  const root = resolve(import.meta.dirname, '../../..');
  const real = areaContext(root, [{ metadata: { runtimeAcceptance: [{ tags: ['C:loading'] }] } }]);
  expect(areasOfPath('runtime/core/svc-rt/src/owner/step.rs', real)).toEqual({ areas: ['C:loading'] });
  expect(areasOfPath('runtime/core/test-support/src/lib.rs', real)).toEqual({ none: true });
  expect(areasOfPath('runtime/core/lifecycle/src/graph.rs', real)).toEqual({ areas: ['C:loading'] });
});
