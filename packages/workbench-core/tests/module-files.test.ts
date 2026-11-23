// The store path of a module's own file (moduleFilePath in src/modules/dktransfer.ts): the rule of dk_data_path in sdk/include/dk/platform/data.h.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { bindModuleFiles } from '../src/module-ui/module-files.ts';
import { createFakeSessions, createFakeTransferApp } from '../src/transfer/fake-app.ts';
import { moduleFilePath } from '../src/modules/dktransfer.ts';

test('a file name goes under data/<module id>/', () => {
  assert.equal(moduleFilePath('tools/bootscreen', 'intro.dkbs'), 'data/tools/bootscreen/intro.dkbs');
});

test('an empty name, a slash or ".." is refused', () => {
  for (const name of ['', 'a/b', '/a', '..', 'a..b', '../x'])
    assert.throws(() => moduleFilePath('tools/bootscreen', name), name);
});

test('a name of 48 characters or more is refused', () => {
  assert.equal(moduleFilePath('tools/bootscreen', 'a'.repeat(47)), `data/tools/bootscreen/${'a'.repeat(47)}`);
  assert.throws(() => moduleFilePath('tools/bootscreen', 'a'.repeat(48)), /shorter than 48/);
});

test('a delayed listing from the previous target cannot replace the current target’s files', async () => {
  let started!: () => void;
  const listing = new Promise<void>((resolve) => {
    started = resolve;
  });
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const sessions = createFakeSessions((key) => ({
    info: async () => ({ runtime: 4, caps: 0 }),
    async list() {
      if (key === 'midi:A') {
        started();
        await pending;
      }
      return [{ path: `data/test/module/${key === 'midi:A' ? 'old' : 'new'}.dkbs`, size: 16 }];
    },
  }));
  const { target } = createFakeTransferApp({ sessions });
  await target.selectTarget({ kind: 'midi', port: 'A' });
  const old = bindModuleFiles('test/module', target.bindSession(), () => true);
  const stale = assert.rejects(old.list(), /target changed/);
  await listing;

  await target.selectTarget({ kind: 'midi', port: 'B' });
  const current = bindModuleFiles('test/module', target.bindSession(), () => true);
  assert.deepEqual(await current.list(), [{ name: 'new.dkbs', size: 16 }]);
  finish();
  await stale;
});

test.each(['upload', 'delete'] as const)('%s waiting on INFO stops after switching away and back', async (action) => {
  let started!: () => void;
  const preflight = new Promise<void>((resolve) => {
    started = resolve;
  });
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const writes: string[] = [];
  const sessions = createFakeSessions((key) => ({
    async info() {
      started();
      await pending;
      return { runtime: 4 };
    },
    async put(path: string) {
      writes.push(`${key} ${path}`);
    },
    async delete(path: string) {
      writes.push(`${key} ${path}`);
    },
    async verifyFile() {
      return true;
    },
  }));
  const { target } = createFakeTransferApp({ sessions });
  const origin = { kind: 'midi', port: 'A' } as const;
  await target.selectTarget(origin);
  const old = bindModuleFiles('test/module', target.bindSession(), () => true);
  const write = action === 'upload' ? old.send('intro.dkbs', Uint8Array.of(1), () => {}) : old.remove('intro.dkbs');
  const stale = assert.rejects(write, /target changed/);
  await preflight;

  await target.selectTarget({ kind: 'midi', port: 'B' });
  await target.selectTarget(origin);
  finish();
  await stale;
  assert.deepEqual(writes, []);
});
