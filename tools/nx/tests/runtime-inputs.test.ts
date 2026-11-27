import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { CELLS, firmwareDir, overrideInputs, syxPath } from '../device-inputs.mjs';
import { releaseProfileEnv } from '../cargo-profile-env.mjs';
import { deviceInputs, forDevice } from '../device-test-keys.mjs';
import { cellShorts, filesOf, nonCellGlobs } from '../device-files.mjs';

const scratch = mkdtempSync(join(tmpdir(), 'digikit-nx-runtime-inputs-'));
afterAll(() => rmSync(scratch, { recursive: true, force: true }));

describe('device override cache inputs', () => {
  it('keeps the default stable and ignores unrelated environment and files', async () => {
    const before = await overrideInputs({});
    writeFileSync(join(scratch, 'unrelated'), 'changed');
    expect(await overrideInputs({ UNRELATED: 'changed' })).toEqual(before);
  });

  it.each([
    'DIGI_EMUD',
    'DIGI_EMUD_DT2',
    'DIGI_EMUD_DN2',
    'DIGI_PATCHKIT',
    'DIGI_PATCHKIT_BIN',
    'DIGI_PLUSDRIVE',
    'DIGI_STORAGE_FAULT_EMUD',
    'DIGI_LP0_EMUD',
    'DIGI_DSP_PEEK_EMUD',
    'DIGI_DDR_MONITOR_EMUD',
    'DIGI_PATCHED_STARTUP_EMUD',
    'DIGI_STEREO_PROOF_EMUD',
    'DIGI_CATALOG_EMUD',
    'DIGI_FILES_EMUD_DT2',
    'DIGI_FILES_EMUD_DN2',
  ])('invalidates %s for in-place binary changes and follows symlinks', async (key) => {
    const binary = join(scratch, key);
    const link = `${binary}-link`;
    writeFileSync(binary, 'first');
    symlinkSync(binary, link);
    const before = await overrideInputs({ [key]: link });
    expect(await overrideInputs({ [key]: link })).toEqual(before);
    writeFileSync(binary, 'second');
    expect(await overrideInputs({ [key]: link })).not.toEqual(before);
    expect(await overrideInputs({ [key]: binary })).not.toEqual(await overrideInputs({ [key]: link }));
    rmSync(binary);
    const missing = await overrideInputs({ [key]: link });
    expect(missing).not.toEqual(before);
    expect(await overrideInputs({ [key]: link })).toEqual(missing);
  });

  it.each(['PATCHKIT_MODULES', 'DIGI_PK_ROOT', 'DIGI_RUNTIME_MODULES'])(
    'prevents cache reuse for nonempty %s, even at an unchanged or missing path',
    async (key) => {
      const path = join(scratch, `${key}-missing`);
      const before = await overrideInputs({ [key]: path });
      expect(await overrideInputs({ [key]: path })).not.toEqual(before);
      writeFileSync(path, 'changed in place');
      expect(await overrideInputs({ [key]: path })).not.toEqual(before);
      const empty = await overrideInputs({ [key]: '' });
      if (key === 'DIGI_RUNTIME_MODULES') expect(await overrideInputs({ [key]: '' })).not.toEqual(empty);
      else expect(await overrideInputs({ [key]: '' })).toEqual(empty);
    },
  );

  it.each([
    'DIGI_CATALOG_CARD',
    'DIGI_FILES_CARD_DT2',
    'DIGI_FILES_PATCH_DT2',
    'DIGI_FILES_PATCH_DN2',
    'DIGI_FILES_PLAN_DT2',
    'DIGI_FILES_PLAN_DN2',
    'DIGI_LP0_BOOT_FIXTURE',
  ])('prevents reuse with specialized external fixture %s, including an empty path', async (key) => {
    for (const value of ['', join(scratch, 'external-fixture')]) {
      expect(await overrideInputs({ [key]: value })).not.toEqual(await overrideInputs({ [key]: value }));
    }
  });

  it.each(['DIGI_LP0_VOICES', 'DIGI_LP0_CLOSE', 'DIGI_LP0_AUDIO', 'DIGI_LP0_FULL_BANK'])(
    'invalidates when the %s proof selection changes',
    async (key) => {
      const before = await overrideInputs({ [key]: '1' });
      expect(await overrideInputs({ [key]: '1' })).toEqual(before);
      expect(await overrideInputs({ [key]: '0' })).not.toEqual(before);
    },
  );
});

describe('device files in a device test key', () => {
  const merge = (imports: string[]) =>
    deviceInputs(
      'p',
      ['a.device.test.ts'],
      [],
      null,
      () => imports,
      () => [],
      false,
      { merge: true },
    ).inputs;

  it('leaves out a device without a cell, unless the test loads the whole registry', () => {
    const nonCell = nonCellGlobs().find((g) => /^devices\/[^*/]+\.toml$/.test(g));
    if (!nonCell) throw new Error('no device without an acceptance cell in devices/*.toml');
    const excluded = `!{workspaceRoot}/${nonCell}`;
    expect(merge([])).toContain(excluded);
    expect(merge(['{workspaceRoot}/packages/devices/src/node.ts'])).not.toContain(excluded);
  });
});

describe('the inputs of a half', () => {
  const LIVE = '{workspaceRoot}/tests/regression/runtime/patchkit.ts';
  const REGISTRY = '{workspaceRoot}/packages/devices/src/node.ts';
  // "a device" and "another device": two devices with a cell, taken from the device files
  const [mine, other] = cellShorts();
  if (!mine || !other) throw new Error('the half tests need two devices with an acceptance cell in devices/*.toml');
  const keyOf = (opts: object, imports: string[] = [LIVE]) =>
    deviceInputs(
      'p',
      ['a.device.test.ts'],
      [],
      null,
      () => imports,
      () => [],
      false,
      opts,
    ).inputs;
  const outputs = (inputs: unknown[]): string[] =>
    inputs.flatMap((i: any) => (i?.dependentTasksOutputFiles ? [i.dependentTasksOutputFiles] : []));
  const negated = (inputs: unknown[], globs: string[]) => globs.filter((g) => inputs.includes(`!{workspaceRoot}/${g}`));

  it.each([{ merge: true }, { narrow: true }])(
    'a live %o key follows the patchkit key of its device, not the binary',
    (opts) => {
      const half = outputs(forDevice(keyOf(opts), mine));
      expect(half).toContain(`dist/bytes/patchkit-test-${mine}.key`);
      expect(half).not.toContain(`dist/bytes/patchkit-test-${other}.key`);
      expect(half).not.toContain('dist/patchkit/patchkit');
      expect(outputs(forDevice(keyOf(opts), null))).toContain('dist/bytes/patchkit-test-*.key');
    },
  );

  it('a merge half hashes the image and daemon files of its own device only', () => {
    const half = outputs(forDevice(keyOf({ merge: true }, []), mine));
    expect(half).toContain(`dist/bytes/images-${mine}.sha256`);
    expect(half).toContain(`dist/bytes/daemon-test-${mine}.key`);
    expect(half.filter((o) => o.includes(`-${other}.`))).toEqual([]);
    const parent = outputs(forDevice(keyOf({ merge: true }, []), null));
    expect(parent).toContain('dist/bytes/images-*.sha256');
    expect(parent).toContain('dist/bytes/daemon-test-*.key');
  });

  it('a merge half leaves out the files of the other device, unless the test loads the registry', () => {
    const half = forDevice(keyOf({ merge: true }), mine);
    expect(negated(half, filesOf(other))).toEqual(filesOf(other));
    expect(negated(half, filesOf(mine))).toEqual([]);
    expect(negated(forDevice(keyOf({ merge: true }, [LIVE, REGISTRY]), mine), filesOf(other))).toEqual([]);
    expect(negated(forDevice(keyOf({ merge: true }), null), filesOf(other))).toEqual([]);
  });

  it('a full key keeps the binary and every device file', () => {
    const full = keyOf({});
    expect(forDevice(full, mine)).toEqual(full);
    expect(outputs(full)).toContain('dist/patchkit/patchkit');
  });

  it('a merge key of a test that does not run patchkit holds no patchkit input', () => {
    expect(outputs(forDevice(keyOf({ merge: true }, []), mine)).filter((o) => o.includes('patchkit'))).toEqual([]);
  });
});

describe('Cargo release profile cache input', () => {
  it('normalizes key order and includes arbitrary release/build-override options', () => {
    const options = { CARGO_PROFILE_RELEASE_OPT_LEVEL: '1', CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_DEBUG: 'true' };
    expect(releaseProfileEnv(options)).toEqual(
      releaseProfileEnv(Object.fromEntries(Object.entries(options).reverse())),
    );
    expect(releaseProfileEnv(options)).not.toEqual(
      releaseProfileEnv({ ...options, CARGO_PROFILE_RELEASE_OPT_LEVEL: '2' }),
    );
    expect(releaseProfileEnv(options)).not.toEqual(releaseProfileEnv({ CARGO_PROFILE_RELEASE_OPT_LEVEL: '1' }));
    expect(releaseProfileEnv({ ...options, UNRELATED: 'value', CARGO_PROFILE_DEV_DEBUG: 'true' })).toEqual(
      releaseProfileEnv(options),
    );
    expect(releaseProfileEnv({ CARGO_PROFILE_RELEASE_OPT_LEVEL: '' })).not.toEqual(releaseProfileEnv({}));
  });
});

describe('stock firmware folder', () => {
  const root = resolve(import.meta.dirname, '../../..');

  it('is firmware/ of the repository, or DIGI_FIRMWARE_DIR resolved against the repository root', () => {
    expect(firmwareDir({})).toBe(join(root, 'firmware'));
    expect(firmwareDir({ DIGI_FIRMWARE_DIR: '' })).toBe(join(root, 'firmware'));
    expect(firmwareDir({ DIGI_FIRMWARE_DIR: scratch })).toBe(scratch);
    expect(firmwareDir({ DIGI_FIRMWARE_DIR: 'elsewhere/fw' })).toBe(join(root, 'elsewhere/fw'));
  });

  it('holds the stock image of every cell', () => {
    for (const cell of CELLS) expect(syxPath(cell, { DIGI_FIRMWARE_DIR: scratch })).toBe(join(scratch, cell.syx));
  });
});
