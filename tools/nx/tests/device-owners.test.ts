import { expect, it } from 'vitest';
import { ownedConformance, ownerOf, ownershipSelection } from '../device-owners.mjs';

const project = (
  name: string,
  root: string,
  targets: string[],
  runtimeAcceptance: object[],
  subjects: string[] = [],
) => ({
  name,
  root,
  targets,
  subjects,
  metadata: { runtimeAcceptance },
});

// core/runtime with a nested package project, test projects naming their subject, a fixture a test loads, a service and a module.
const graph = [
  project('core/runtime', 'runtime/core', [], []),
  project('core/runtime-package', 'runtime/core/src/rs/src/package', [], []),
  project(
    'core/runtime-tests',
    'runtime/core/tests',
    ['test-device--reload--dt2', 'test-device--reload--dn2', 'test-device--boot--dt2', 'test-device--boot--dn2'],
    [{ files: ['reload.device.test.ts'] }, { files: ['boot.device.test.ts'], sources: ['tools/boot-helper.py'] }],
    ['core/runtime-package'],
  ),
  project(
    'core/runtime-boot-tests',
    'runtime/core/src/rs/src/boot-tests',
    ['test-device--dt2'],
    [{ files: ['boot.device.test.ts'] }],
    ['core/runtime'],
  ),
  project('test/local-ping', 'tests/fixtures/modules/local-ping', [], []),
  project(
    'core/runtime-provider-tests',
    'runtime/core/tests/providers',
    ['test-device--dt2', 'test-device--dn2'],
    [{ files: ['providers.device.test.ts'], modules: ['test/local-ping'] }],
  ),
  project('core/services/lfo', 'services/lfo', [], []),
  project(
    'lfo-tests',
    'services/lfo/tests/device',
    ['test-device--dt2'],
    [{ files: ['lfo.device.test.ts'] }],
    ['core/services/lfo'],
  ),
  project(
    'machines-user-tests',
    'tests/consumer',
    ['test-device--dt2'],
    [{ files: ['consumer.device.test.ts'], modules: ['core/services/lfo'] }],
  ),
  project('tb303', 'modules/tb303', ['test-device--dt2', 'test-conformance'], [{ files: ['tests/tb.device.test.ts'] }]),
  project('other', 'modules/other', ['test-conformance'], []),
  project('regression', 'tests/regression', [], []),
  project('@digi/dk-session', 'packages/dk-session', [], []),
];

const select = (changed: string[], extra: object = {}) => {
  const got = ownershipSelection(graph, {
    changed,
    closureOf: (root: string, file: string) =>
      `${root}/${file}` === 'runtime/core/tests/reload.device.test.ts'
        ? ['tests/regression/runtime/kit.ts', 'packages/dk-session/src/host.ts', 'packages/dk-session/src/gen_proto.ts']
        : [],
    portUsers: (f: string) =>
      f === 'ports/fw/lfo.toml' ? ['core/services/lfo'] : f === 'ports/fw/x.toml' ? ['nobody'] : [],
    deviceOf: (f: string) => (f === 'devices/dt2.toml' ? 'dt2' : undefined),
    ...extra,
  });
  return { got, files: got.tests.map((t) => `${t.project}:${t.file}`) };
};

it('picks the deepest project root, so a nested project wins over its parent', () => {
  expect(ownerOf('runtime/core/src/rs/src/package/loader/x.rs', graph)?.name).toBe('core/runtime-package');
  expect(ownerOf('runtime/core/src/rs/src/boot.rs', graph)?.name).toBe('core/runtime');
  expect(ownerOf('docs/a.txt', graph)).toBeUndefined();
});

it('selects the tests that name the touched project, not those of its parent', () => {
  const { files } = select(['runtime/core/src/rs/src/package/loader/x.rs']);
  expect(files).toEqual(['core/runtime-tests:reload.device.test.ts', 'core/runtime-tests:boot.device.test.ts']);
  expect(select(['runtime/core/src/rs/src/boot.rs']).files).toEqual(['core/runtime-boot-tests:boot.device.test.ts']);
});

it('selects the tests of a project for its own product file, and the service tests that name the service', () => {
  expect(select(['modules/tb303/src/x.c']).files).toEqual(['tb303:tests/tb.device.test.ts']);
  expect(select(['services/lfo/dks/src/lib.rs']).files).toEqual(['lfo-tests:lfo.device.test.ts']);
});

it('does not select a test for a service or real module it only loads', () => {
  expect(select(['services/lfo/dks/src/lib.rs']).files).toEqual(['lfo-tests:lfo.device.test.ts']);
});

it('selects the tests that load a fixture', () => {
  expect(select(['tests/fixtures/modules/local-ping/src/module.c']).files).toEqual([
    'core/runtime-provider-tests:providers.device.test.ts',
  ]);
});

it('selects the tests that import a changed harness file, and leaves driver code to its host tests', () => {
  expect(select(['tests/regression/runtime/kit.ts']).files).toEqual(['core/runtime-tests:reload.device.test.ts']);
  expect(select(['packages/dk-session/src/host.ts']).files).toEqual([]);
  expect(select(['packages/dk-session/src/gen_proto.ts']).files).toEqual(['core/runtime-tests:reload.device.test.ts']);
});

it('selects a record by its listed test file and by its sources', () => {
  expect(select(['runtime/core/tests/boot.device.test.ts']).files).toEqual(['core/runtime-tests:boot.device.test.ts']);
  expect(select(['tools/boot-helper.py']).files).toEqual(['core/runtime-tests:boot.device.test.ts']);
});

it('maps a firmware declaration to the projects of its users', () => {
  expect(select(['ports/fw/lfo.toml']).files).toEqual(['lfo-tests:lfo.device.test.ts']);
  const bad = select(['ports/fw/x.toml', 'ports/fw/none.toml']).got;
  expect(bad.tests).toEqual([]);
  expect(bad.unowned.map((u: { file: string }) => u.file)).toEqual(['ports/fw/x.toml', 'ports/fw/none.toml']);
});

it('names the device of a device file and selects no test for it', () => {
  const { got } = select(['devices/dt2.toml']);
  expect(got.devices).toEqual(['dt2']);
  expect(got.tests).toEqual([]);
});

it('selects nothing for a path no project owns, prints it, and ignores Markdown and project.json', () => {
  const { got } = select(['stray/file.bin', 'docs/a.md', 'modules/tb303/project.json']);
  expect(got.tests).toEqual([]);
  expect(got.unowned).toEqual([{ file: 'stray/file.bin', why: 'no project owns it' }]);
});

it('lists a touched project that no test names', () => {
  const { got } = select(['modules/other/src/x.c']);
  expect(got.tests).toEqual([]);
  expect(got.quiet).toEqual([{ file: 'modules/other/src/x.c', project: 'other' }]);
});

it('runs conformance only for the modules whose own source changed', () => {
  const affected = new Set(['tb303:test-conformance', 'other:test-conformance']);
  expect(ownedConformance(affected, graph, ['modules/tb303/src/x.c'])).toEqual(['tb303:test-conformance']);
  expect(ownedConformance(affected, graph, ['runtime/core/src/rs/src/boot.rs'])).toEqual([]);
  expect(ownedConformance(affected, graph, ['modules/other/README.md'])).toEqual([]);
});
