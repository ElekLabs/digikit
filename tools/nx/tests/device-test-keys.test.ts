import { expect, it } from 'vitest';
import { deviceInputs, withoutDriver } from '../device-test-keys.mjs';
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
  expect(withoutDriver(imports)).toEqual(rest);
});

const keyOf = (opts: object) =>
  deviceInputs(
    'modules/a',
    ['tests/x.device.test.ts'],
    [],
    undefined,
    () => imports,
    () => [],
    false,
    opts,
  ).inputs;

it('test-conformance leaves the driver out, test-conformance-all keeps it', () => {
  const inputs = (full: boolean) =>
    conformanceTarget('.', 'modules/a', { id: 'a' }, 'm.toml', () => imports, full).inputs.filter(
      (i: unknown) => typeof i === 'string' && i.includes('packages/dk-session'),
    );
  expect(inputs(false)).toEqual([]);
  expect(inputs(true)).toHaveLength(1);
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
