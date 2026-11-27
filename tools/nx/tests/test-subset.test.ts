import { expect, it } from 'vitest';
import { DEVICES, DEVICE_SMOKE, halvesOfDevices, namedSelection, selectionPairs } from '../test-subset.mjs';

const project = (name: string, targets: string[], runtimeAcceptance: object[], subjects: string[] = []) => ({
  name,
  targets,
  subjects,
  metadata: { runtimeAcceptance },
});

const [d1, d2, d3] = DEVICES;
const graph = [
  project('core/runtime', [], []),
  project(
    'core/rt-tests',
    [`test-device--boot--${d1}`, `test-device--boot--${d2}`, `test-device--storage--${d1}`],
    [{ files: ['boot.device.test.ts'] }, { files: ['storage.device.test.ts'], dsp: 'sharc' }],
    ['core/runtime'],
  ),
  project('core/m', [`test-device--${d1}`], [{ files: ['tests/m.device.test.ts'] }]),
  project('gate-device-smoke', ['device'], []),
];

it('selects the own device tests of a named project and the tests that name it', () => {
  const got = namedSelection(['core/runtime', 'core/m'], graph);
  expect(got.unknown).toEqual([]);
  expect(selectionPairs(got.tests, graph, [DEVICE_SMOKE]).map((p) => `${p.project}:${p.target}`)).toEqual([
    `core/rt-tests:test-device--boot--${d1}`,
    `core/rt-tests:test-device--boot--${d2}`,
    `core/rt-tests:test-device--storage--${d1}`,
    `core/m:test-device--${d1}`,
    'gate-device-smoke:device',
  ]);
  expect(got.tests.find((t) => t.file === 'storage.device.test.ts')?.dsp).toBe('sharc');
});

it('reports a name that is no project or selects no device test', () => {
  expect(namedSelection(['core/runtime', 'nope', 'gate-device-smoke'], graph).unknown).toEqual([
    'nope',
    'gate-device-smoke',
  ]);
});

it('keeps the halves of the given devices only', () => {
  const halves = [`a:test-device--x--${d1}`, `a:test-device--x--${d3}`, `b:test-device--${d3}`];
  expect(halvesOfDevices(halves, [d3])).toEqual([`a:test-device--x--${d3}`, `b:test-device--${d3}`]);
  expect(halvesOfDevices(halves, null)).toEqual(halves);
});
