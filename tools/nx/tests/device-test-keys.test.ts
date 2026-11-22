import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, matchesGlob } from 'node:path';
import { afterAll, expect, it } from 'vitest';
import { OTHER_DEVICE_FILES, deviceInputs, forDevice, subjectInputs, withoutDriver } from '../device-test-keys.mjs';
import { cellShorts, filesOf } from '../device-files.mjs';
import { conformanceTarget } from '../device-tests.mjs';
import { testSourceGlobs } from '../test-source.mjs';

const imports = [
  '{workspaceRoot}/packages/dk-session/src/dkupload-core.ts',
  '{workspaceRoot}/packages/test-support/src/session.ts',
  '{workspaceRoot}/packages/control-client/src/index.ts',
  '{workspaceRoot}/tests/regression/runtime/fixtures.ts',
  '{workspaceRoot}/modules/a/tests/x.device.test.ts',
];
const rest = imports.slice(3);

it('leaves the driver packages out of a list and keeps the harness helpers', () => {
  expect(withoutDriver(imports)).toEqual([imports[1], ...rest]);
});

it('keeps the driver files that hold device facts', () => {
  const kept = withoutDriver([
    '{workspaceRoot}/packages/dk-session/src/gen_proto.ts',
    '{workspaceRoot}/packages/test-support/src/session.ts',
    '{workspaceRoot}/packages/dk-session/src/dkupload-core.ts',
  ]);
  expect(kept).toEqual([
    '{workspaceRoot}/packages/dk-session/src/gen_proto.ts',
    '{workspaceRoot}/packages/test-support/src/session.ts',
  ]);
});

const keyOf = (opts: object, recs: object[] = []) =>
  deviceInputs(
    'modules/a',
    ['tests/x.device.test.ts'],
    recs,
    undefined,
    () => imports,
    () => [],
    false,
    opts,
  ).inputs;

it('test-conformance leaves the driver out, test-conformance-all keeps it', () => {
  const inputs = (full: boolean) =>
    conformanceTarget('.', 'modules/a', { id: 'a' }, 'm.toml', () => imports, full).inputs;
  const drivers = (full: boolean) =>
    inputs(full).filter((i: unknown) => typeof i === 'string' && i.includes('packages/dk-session'));
  expect(drivers(false)).toEqual([]);
  expect(drivers(true)).toHaveLength(1);
  expect(inputs(false)).toContainEqual({ env: 'DIGI_DEVICE' });
  expect(inputs(true)).toContainEqual({ env: 'DIGI_DEVICE' });
});

it('keeps the driver files in a full key only', () => {
  const driver = (inputs: unknown[]) =>
    inputs.filter((i) => typeof i === 'string' && i.includes('packages/dk-session'));
  expect(driver(keyOf({ merge: true }))).toEqual([]);
  expect(driver(keyOf({ narrow: true }))).toEqual([]);
  expect(driver(keyOf({}))).toHaveLength(1);
  for (const opts of [{ merge: true }, { narrow: true }, {}])
    expect(keyOf(opts)).toContain('{workspaceRoot}/tests/regression/runtime/fixtures.ts');
});

const outputGlobs = (inputs: unknown[]) =>
  inputs.flatMap((i) =>
    typeof (i as { dependentTasksOutputFiles?: string })?.dependentTasksOutputFiles === 'string'
      ? [(i as { dependentTasksOutputFiles: string }).dependentTasksOutputFiles]
      : [],
  );

it('a narrow key follows the built bytes of its device and leaves the built sources out', () => {
  const inputs = keyOf({ narrow: true, base: '.' });
  expect(outputGlobs(inputs)).toEqual(
    expect.arrayContaining(['dist/bytes/images-{device}.sha256', 'dist/bytes/daemon-test-{device}.key']),
  );
});

const MERGE_BYTES = ['dist/bytes/images-*.sha256', 'dist/bytes/daemon-test-*.key'];
const moduleFiles = (inputs: unknown[]) =>
  inputs.filter(
    (i) =>
      typeof i === 'string' &&
      i.startsWith('{workspaceRoot}/modules/a/') &&
      !i.includes('/tests/') &&
      !i.endsWith('/module.toml'),
  );

it('a merge, narrow or conformance key holds the built bytes and no source file of the module', () => {
  const conformance = (full: boolean) =>
    conformanceTarget('.', 'modules/a', { id: 'a' }, 'modules/a/module.toml', () => imports, full).inputs;
  const keys = [keyOf({ merge: true }), keyOf({ narrow: true }), conformance(false)];
  for (const inputs of keys) {
    // no `production` of the module, whose files would include its src, include, fit and sdk directories
    expect(inputs).not.toContain('production');
    expect(moduleFiles(inputs)).toEqual([]);
    expect(outputGlobs(forDevice(inputs, null))).toEqual(expect.arrayContaining(MERGE_BYTES));
  }
  // a full key keeps the module's production files
  expect(keyOf({})).toContain('production');
  expect(conformance(true)).toContain('production');
});

it('the files a record lists in sources are inputs of every key', () => {
  const source = 'tests/fixtures/modules/sine/src/dsp/sine.s';
  const recs = [{ sources: [source] }];
  for (const opts of [{ merge: true }, { narrow: true }, {}])
    expect(keyOf(opts, recs)).toContain(`{workspaceRoot}/${source}`);
  const conformance = conformanceTarget('.', 'modules/a', { id: 'a' }, 'modules/a/module.toml', () => imports).inputs;
  expect(conformance).toContain('{workspaceRoot}/modules/a/module.toml');
  expect(moduleFiles(keyOf({ narrow: true }))).toEqual(moduleFiles(keyOf({ narrow: true }, [{}])));
});

it('a record that compiles at run time keeps the SDK in every key, the narrow one included', () => {
  const sdk = (inputs: unknown[]) => inputs.filter((i) => typeof i === 'string' && i.includes('/sdk/'));
  for (const opts of [{ merge: true }, { narrow: true }, {}])
    expect(sdk(keyOf(opts, [{ compiles: true }]))).not.toEqual([]);
  expect(sdk(keyOf({ narrow: true }, [{}]))).toEqual([]);
});

it('a narrow key of a packed subject follows its pack and not its sources', () => {
  const subject = { root: 'modules/a', devices: [] as string[], pack: true };
  const inputs = keyOf({
    narrow: true,
    subjects: ['x/a'],
    subjectRoots: { 'x/a': subject },
    services: new Set(['a']),
    base: '.',
  });
  expect(outputGlobs(forDevice(inputs, null))).toContain('dist/modules/*/a.dks');
  expect(forDevice(inputs, null)).not.toContainEqual({ input: 'production', projects: ['x/a'] });
});

it('a narrow key of a subject without objects or pack keeps its production files', () => {
  const inputs = forDevice(keyOf({ narrow: true, subjects: ['x/a'], base: '.' }), null);
  expect(inputs).toContainEqual({ input: 'production', projects: ['x/a'] });
});

it('test-conformance follows the patchkit key and not the patchkit binary; the -all key keeps the binary', () => {
  const globs = (full: boolean) =>
    outputGlobs(conformanceTarget('.', 'modules/a', { id: 'a' }, 'm.toml', () => imports, full).inputs);
  expect(globs(false)).not.toContain('dist/patchkit/patchkit');
  expect(globs(false).some((g) => g.startsWith('dist/bytes/patchkit-test-'))).toBe(true);
  expect(globs(true)).toContain('dist/patchkit/patchkit');
});

it('a half follows the image of a subject built into it and the pack or production files of the others', () => {
  const key = (subject: object) =>
    keyOf({ narrow: true, subjects: ['x/a'], subjectRoots: { 'x/a': subject }, services: new Set(['a']), base: '.' });
  const production = { input: 'production', projects: ['x/a'] };
  const files = (inputs: unknown[]) => inputs.filter((i) => typeof i === 'string' && i.includes('modules/x-a'));
  // devices and pack: both halves hold the fileset and the pack, none holds production
  const both = key({ root: 'modules/x-a', devices: ['p'], pack: true });
  for (const device of ['p', 'q']) {
    expect(outputGlobs(forDevice(both, device))).toContain('dist/modules/*/a.dks');
    expect(files(forDevice(both, device))).toContain('{workspaceRoot}/modules/x-a/**/*');
    expect(forDevice(both, device)).not.toContainEqual(production);
  }
  // devices without a pack: the half of the image holds the fileset, the other half the production files
  const image = key({ root: 'modules/x-a', devices: ['p'], pack: false });
  expect(files(forDevice(image, 'p'))).toContain('{workspaceRoot}/modules/x-a/**/*');
  expect(forDevice(image, 'p')).not.toContainEqual(production);
  expect(forDevice(image, 'q')).toContainEqual(production);
  expect(files(forDevice(image, 'q'))).toEqual([]);
  // the manifests of the crate are not inputs of a packed subject
  expect(files(forDevice(both, 'p'))).toContain('!{workspaceRoot}/modules/x-a/Cargo.{toml,lock}');
});

it('a key that holds a pack holds its sidecars; a narrow key holds the device and port files', () => {
  const narrow = outputGlobs(
    forDevice(keyOf({ narrow: true, services: new Set(['a']) }, [{ modules: ['x/a', 'x/b'] }]), null),
  );
  for (const leaf of ['a', 'b']) {
    expect(narrow).toContain(`dist/modules/*/${leaf}.services.json`);
    expect(narrow).toContain(`dist/modules/*/${leaf}.addrmap.json`);
  }
  for (const opts of [{ merge: true }, {}])
    expect(outputGlobs(forDevice(keyOf(opts), null))).toEqual(
      expect.arrayContaining(['dist/modules/*/*.services.json', 'dist/modules/*/*.addrmap.json']),
    );
  const files = forDevice(keyOf({ narrow: true }), null);
  expect(files).toContain('{workspaceRoot}/devices/**/*.toml');
  expect(files).toContain('{workspaceRoot}/ports/*.toml');
  // a narrow half, like a merge half, leaves the other devices' files out (expanded per device by forDevice)
  for (const opts of [{ narrow: true }, { merge: true }]) expect(keyOf(opts)).toContain(OTHER_DEVICE_FILES);
});

// Nx unions the positive filesets and then removes every negated one, wherever it stands in the list.
const holds = (inputs: unknown[], file: string) => {
  const globs = inputs.filter((i): i is string => typeof i === 'string' && /^!?\{workspaceRoot\}\//.test(i));
  const match = (g: string) => matchesGlob(file, g.replace('{workspaceRoot}/', '').replace(/^!/, ''));
  return globs.some((g) => !g.startsWith('!') && match(g)) && !globs.some((g) => g.startsWith('!') && match(g));
};

const temp = mkdtempSync(join(tmpdir(), 'device-keys-'));
afterAll(() => rmSync(temp, { recursive: true, force: true }));
for (const f of [
  'runtime/core/src/a.c',
  'runtime/core/tests/capacity/commit.device.test.ts',
  'runtime/core/tests/capacity/capacity.device.test.ts',
  'runtime/core/tests/capacity/helpers.ts',
  'runtime/core/tests/other.device.test.ts',
  'runtime/core/src/x/tests/unit.ts',
  'runtime/core/notes.txt',
  'runtime/core/tests/capacity/project.json',
  'runtime/core/tests/lfo/project.json',
  'runtime/core/tests/lfo/lfo.device.test.ts',
  'runtime/core/tests/reload.device.test.ts',
  'runtime/core/tests/setup.ts',
]) {
  mkdirSync(join(temp, f, '..'), { recursive: true });
  writeFileSync(join(temp, f), '');
}

it('the own test file and setup of a test project inside its subject stay in the key; siblings and other tests stay out', () => {
  const own = 'runtime/core/tests/capacity';
  const test = `${own}/commit.device.test.ts`;
  const inputs = deviceInputs(
    own,
    ['commit.device.test.ts'],
    [],
    undefined,
    () => [`{workspaceRoot}/${test}`],
    () => [],
    false,
    {
      narrow: true,
      siblingTests: ['capacity.device.test.ts'],
      subjects: ['core/runtime'],
      subjectRoots: { 'core/runtime': { root: 'runtime/core', devices: [], pack: true } },
      base: temp,
    },
  ).inputs;
  const flat = [
    ...inputs.filter((i) => typeof i === 'string'),
    ...forDevice(inputs, null).filter((i) => typeof i === 'string'),
  ];
  expect(holds(flat, test)).toBe(true);
  expect(holds(flat, `${own}/helpers.ts`)).toBe(true);
  expect(holds(flat, `${own}/capacity.device.test.ts`)).toBe(false);
  expect(holds(flat, 'runtime/core/tests/other.device.test.ts')).toBe(false);
  expect(holds(flat, 'runtime/core/src/x/tests/unit.ts')).toBe(false);
  expect(holds(flat, 'runtime/core/notes.txt')).toBe(true);
});

it('a test project that holds nested test projects leaves their files out of its key and keeps its own', () => {
  const root = 'runtime/core/tests';
  const test = `${root}/reload.device.test.ts`;
  const inputs = deviceInputs(
    root,
    ['reload.device.test.ts'],
    [],
    undefined,
    () => [`{workspaceRoot}/${test}`],
    () => [],
    false,
    {
      narrow: true,
      subjects: ['core/runtime'],
      subjectRoots: { 'core/runtime': { root: 'runtime/core', devices: [], pack: true } },
      base: temp,
    },
  ).inputs;
  const flat = [
    ...inputs.filter((i) => typeof i === 'string'),
    ...forDevice(inputs, null).filter((i) => typeof i === 'string'),
  ];
  expect(holds(flat, test)).toBe(true);
  expect(holds(flat, `${root}/setup.ts`)).toBe(true);
  for (const f of ['capacity/commit.device.test.ts', 'capacity/helpers.ts', 'lfo/lfo.device.test.ts'])
    expect(holds(flat, `${root}/${f}`)).toBe(false);
});

it('subjectInputs without a test project inside leaves every tests directory out', () => {
  const inputs = subjectInputs('runtime/core', temp);
  expect(holds(inputs, 'runtime/core/tests/capacity/helpers.ts')).toBe(false);
  expect(holds(inputs, 'runtime/core/notes.txt')).toBe(true);
});

it('every key kind holds the own test file of its project', () => {
  const test = 'modules/a/tests/x.device.test.ts';
  for (const opts of [
    { merge: true },
    { narrow: true },
    {},
    { narrow: true, siblingTests: ['tests/y.device.test.ts'] },
  ])
    expect(holds(keyOf(opts), test)).toBe(true);
  const conformance = conformanceTarget('.', 'modules/a', { id: 'a' }, 'm.toml', () => imports, false).inputs;
  expect(holds(conformance as unknown[], test)).toBe(true);
});

const nestedKey = () =>
  deviceInputs(
    'runtime/core/tests/capacity',
    ['commit.device.test.ts'],
    [],
    undefined,
    () => ['{workspaceRoot}/runtime/core/tests/capacity/commit.device.test.ts'],
    () => [],
    false,
    {
      narrow: true,
      siblingTests: ['capacity.device.test.ts'],
      subjects: ['core/runtime'],
      subjectRoots: { 'core/runtime': { root: 'runtime/core', devices: [], pack: true } },
      base: temp,
    },
  ).inputs;

it('the half of a device in a nested test project leaves the files of every other device out; every half holds the own file', () => {
  const own = 'runtime/core/tests/capacity/commit.device.test.ts';
  const shorts = cellShorts();
  expect(shorts.length).toBeGreaterThan(1);
  for (const short of shorts) {
    const half = forDevice(nestedKey(), short);
    expect(holds(half, own)).toBe(true);
    expect(holds(half, 'runtime/core/tests/capacity/capacity.device.test.ts')).toBe(false);
    // the half holds every file of its device and excludes the device file and ports of each other device
    for (const f of filesOf(short)) expect(holds(half, f)).toBe(true);
    for (const other of shorts.filter((o) => o !== short))
      for (const f of filesOf(other)) expect(holds(half, f)).toBe(false);
  }
});

it('the conformance key of a nested test project holds none of its test files and none of the subject tests', () => {
  const root = 'runtime/core/tests/capacity';
  for (const full of [false, true]) {
    const inputs = conformanceTarget('.', root, { id: 'x' }, 'm.toml', () => [], full).inputs as unknown[];
    for (const f of ['capacity.device.test.ts', 'commit.device.test.ts', 'capacity-helpers.ts'])
      expect(holds(inputs, `${root}/${f}`)).toBe(false);
    expect(holds(inputs, 'runtime/core/tests/reload.device.test.ts')).toBe(false);
  }
});

it('a file added under a tests directory of the subject is no input of a nested project but the listed entries are (snapshot)', () => {
  const flat = [
    ...nestedKey().filter((i) => typeof i === 'string'),
    ...forDevice(nestedKey(), null).filter((i) => typeof i === 'string'),
  ];
  expect(holds(flat, 'runtime/core/tests/other.device.test.ts')).toBe(false);
  // a name below the own root is allowed (positive glob), a tests path of src stays out
  expect(holds(flat, 'runtime/core/tests/capacity/new-helper.ts')).toBe(true);
  expect(holds(flat, 'runtime/core/src/y/tests/z.ts')).toBe(false);
});

it('the rule globs are filtered by path segment: a subject root that has a tests segment keeps the other test globs', () => {
  const root = 'runtime/tests/core';
  const inputs = subjectInputs(root, temp, `${root}/tests/capacity`);
  const rule = testSourceGlobs(`{workspaceRoot}/${root}`).filter((g) => !g.endsWith('/**/tests/**/*'));
  expect(rule.length).toBeGreaterThan(0);
  for (const g of rule) expect(inputs).toContain(g);
});
