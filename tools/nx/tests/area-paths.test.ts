import { expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { areaContext, areasOfPath, crateOf, modulePathOf, ownerOf, parseOwners, OWNERS_FILE } from '../area-paths.mjs';

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
  expect(areasOfPath('runtime/core/src/rs/src/entry.rs', ctx)).toEqual({ shared: true });
});

it('gives a test source, a header, a table and a service without records no area', () => {
  expect(areasOfPath('runtime/core/src/rs/src/boot/tests.rs', ctx)).toEqual({ none: true });
  expect(areasOfPath('runtime/core/src/rs/src/fake_platform.rs', ctx)).toEqual({ none: true });
  expect(areasOfPath('runtime/core/abi.toml', ctx)).toEqual({ none: true });
  expect(areasOfPath('sdk/include/dk.h', ctx)).toEqual({ none: true });
  expect(areasOfPath('services/fx/src/lib.rs', ctx)).toEqual({ none: true });
});

it('maps the real runtime files by the real owners.toml and service records', () => {
  const root = resolve(import.meta.dirname, '../../..');
  const real = areaContext(root, [{ metadata: { runtimeAcceptance: [{ tags: ['S:machines'] }] } }]);
  expect(real.owners.length).toBeGreaterThan(20);
  expect(readFileSync(resolve(root, OWNERS_FILE), 'utf8')).toContain('"C:boot"');
  expect(areasOfPath('runtime/core/src/rs/src/boot.rs', real)).toEqual({ areas: ['C:boot'] });
  expect(areasOfPath('runtime/core/src/rs/src/loader/mod.rs', real)).toEqual({ areas: ['C:loading'] });
  expect(areasOfPath('runtime/core/src/rs/src/entry.rs', real)).toEqual({ shared: true });
  expect(areasOfPath('services/machines/src/lib.rs', real)).toEqual({ areas: ['S:machines'] });
});
