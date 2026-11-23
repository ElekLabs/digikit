import { expect, it } from 'vitest';
import { parseOwners } from '../area-paths.mjs';
import { DEVICES, DEVICE_SMOKE, areaSelection, halvesOfDevices, selectionPairs } from '../test-subset.mjs';

const project = (name: string, targets: string[], runtimeAcceptance: object[]) => ({
  name,
  targets,
  metadata: { runtimeAcceptance },
});

const [d1, d2, d3] = DEVICES;
// The area gate: changed paths name areas, the tests of those areas run, the smoke set is fixed, the rest is skipped.
const areaGraph = [
  { ...project('core/runtime', [], []), root: 'runtime/core' },
  {
    ...project(
      'core/rt-tests',
      [
        `test-device--boot--${d1}`,
        `test-device--boot--${d2}`,
        `test-device--storage--${d1}`,
        `test-device--storage--${d2}`,
      ],
      [
        { files: ['boot.device.test.ts'], tags: ['C:boot'] },
        { files: ['storage.device.test.ts'], tags: ['C:storage'] },
      ],
    ),
    root: 'runtime/core/tests',
    subjects: ['core/runtime'],
  },
  {
    ...project('core/m', [`test-device--${d1}`], [{ files: ['tests/m.device.test.ts'], tags: ['C:state'] }]),
    root: 'modules/m',
  },
  { ...project('gate-device-smoke', ['device'], []), root: 'tools/gate/device-smoke' },
  { ...project('patchkit', [], []), root: 'packages/patchkit' },
  { ...project('core/m', ['test-conformance'], []), root: 'modules/m' },
  { ...project('core/other', ['test-conformance'], []), root: 'modules/other' },
];
const areaCtx = {
  owners: parseOwners('shared = ["dkrt::entry"]\n"C:boot" = ["dkrt::boot"]\n"C:storage" = ["dkrt::storage"]'),
  serviceTags: new Set<string>(),
  recordTags: new Set(['S:lfo']),
  portUsers: (file: string) => (file === 'ports/fw/lfo.toml' ? ['core/services/lfo'] : []),
  deviceOf: (file: string) => (file === 'devices/x-device.toml' ? d3 : undefined),
  crateOf: (file: string) =>
    file.startsWith('runtime/core/src/rs/') ? { name: 'dkrt', dir: 'runtime/core/src/rs' } : undefined,
};
// A stand-in for Nx's answer (device-reach.mjs): these paths are inputs of device tests, the others are not.
const fakeReaches = (files: string[]) =>
  files.filter(
    (f) =>
      /^(packages\/patchkit\/|sdk\/|runtime\/core\/owners|h\/)/.test(f) ||
      /^runtime\/core\/src\/rs\/src\/(newmod|entry)\.rs$/.test(f),
  );
const selected = (changed: string[], tags: string[] = [], closureOf?: (r: string, f: string) => string[]) => {
  const got = areaSelection(tags, areaGraph, { changed, ctx: areaCtx, closureOf, reaches: fakeReaches });
  return {
    got,
    pairs: selectionPairs(got.tests, areaGraph, [DEVICE_SMOKE]).map((p) => `${p.project}:${p.target}`),
  };
};

it('selects the tests of the area of a runtime-only change, not of every record that depends on the runtime', () => {
  const { got, pairs } = selected(['runtime/core/src/rs/src/boot.rs']);
  expect(got.mapped).toEqual([{ file: 'runtime/core/src/rs/src/boot.rs', areas: ['C:boot'] }]);
  expect(pairs).toEqual([
    `core/rt-tests:test-device--boot--${d1}`,
    `core/rt-tests:test-device--boot--${d2}`,
    'gate-device-smoke:device',
  ]);
});

it('goes wide for a path with no area that reaches device tests, whatever else changed', () => {
  for (const file of ['packages/patchkit/src/x.rs', 'runtime/core/owners.toml', 'sdk/sdk.toml']) {
    const { got } = selected(['runtime/core/src/rs/src/boot.rs', file]);
    expect(got.wideAll, file).toBe(true);
    expect(got.wide.map((w: { file: string }) => w.file)).toEqual([file]);
  }
  // an unowned file of the runtime is wide, not smoke only
  expect(selected(['runtime/core/src/rs/src/newmod.rs']).got.wideAll).toBe(true);
});

it('selects no device test for a docs-only change', () => {
  const { got, pairs } = selected(['docs/x.md', 'apps/web/a.ts']);
  expect(got.wideAll).toBe(false);
  expect(got.tests).toEqual([]);
  expect(pairs.filter((p) => !p.startsWith('gate-'))).toEqual([]);
});

it('maps the port file of one plug-in point to its area, not to wide', () => {
  const { got } = selected(['ports/fw/lfo.toml']);
  expect(got.wideAll).toBe(false);
  expect(got.mapped).toEqual([{ file: 'ports/fw/lfo.toml', areas: ['S:lfo'] }]);
});

it('widens a device file to the halves of its device only', () => {
  const { got } = selected(['devices/x-device.toml']);
  expect(got.wideAll).toBe(true);
  expect(got.wideDevices).toEqual([d3]);
  const halves = [`a:test-device--x--${d1}`, `a:test-device--x--${d3}`, `b:test-device--${d3}`];
  expect(halvesOfDevices(halves, got.wideDevices)).toEqual([`a:test-device--x--${d3}`, `b:test-device--${d3}`]);
  // with a path that has no area the device filter is gone
  expect(selected(['devices/x-device.toml', 'sdk/sdk.toml']).got.wideDevices).toBeNull();
  expect(halvesOfDevices(halves, null)).toEqual(halves);
});
