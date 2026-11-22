import { expect, it } from 'vitest';
import { deviceInputs, forDevice, withoutDriver } from '../device-test-keys.mjs';
import { conformanceTarget } from '../device-tests.mjs';

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
