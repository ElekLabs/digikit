import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'smol-toml';
import { test } from 'vitest';
import { PatchKit } from '@digi/workbench-core/patch/patchkit-core.ts';
import { fmtVersion } from '@digi/dk-session';
import { shipSystemPackage } from '../scripts/gen-modules.mjs';
import { readBytes, root, wasmPath, workDir } from './support/patchkit-env';

test('the site ships a complete source-built system package with its runtime version', () => {
  const out = workDir('site-system-package-');
  shipSystemPackage(out, root);
  const file = readFileSync(join(out, 'system-package.bin'));
  const sdk = parse(readFileSync(join(root, 'sdk/sdk.toml'), 'utf8')) as { runtime: { version: string } };
  assert.equal(fmtVersion(file.readUInt32BE(0)), sdk.runtime.version);
  const pkg = file.subarray(4);
  assert.equal(pkg.subarray(0, 4).toString(), 'DKSP');
  assert.equal(pkg.readUInt32BE(12), pkg.length - 48);
  assert.deepEqual(pkg.subarray(16, 48), createHash('sha256').update(pkg.subarray(48)).digest());
});

test.skipIf(!existsSync(wasmPath))(
  'browser package planning preserves native installation order and refusals',
  async () => {
    const kit = await PatchKit.create(readBytes(wasmPath), '{"ports":[],"modules":[]}');
    const plan = (repair: boolean, moduleSupport: boolean, verifiedPackageOnCard: boolean) =>
      kit.systemPackagePlan({ repair, moduleSupport, verifiedPackageOnCard });
    assert.deepEqual(plan(false, false, false), ['put', 'verify', 'restart', 'confirm_support']);
    assert.deepEqual(plan(true, false, false), ['put', 'verify', 'restart', 'confirm_support']);
    assert.deepEqual(plan(true, false, true), ['retry', 'restart', 'confirm_support']);
    assert.deepEqual(plan(false, true, false), ['put', 'verify', 'restart', 'wait_for_boot', 'confirm_support']);
    assert.deepEqual(plan(false, true, true), []);
    assert.throws(() => plan(false, false, true), /explicit recovery retry/);
    assert.throws(() => plan(true, true, false), /already has module support/);
    assert.throws(() => plan(true, true, true), /already has module support/);
  },
);
