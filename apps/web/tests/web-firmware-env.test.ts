// The site's Firmware environment (apps/web/src/firmware/web-env.ts) over a fake patch worker, a stub wasm source and a stub kit: no wasm is run.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createWebFirmwareEnv } from '../src/firmware/web-env.ts';
import type { PatchKitLike, PreparedFlash } from '@digi/workbench-core/patch/patchkit-core.ts';
import { memoryWasmSource } from '@digi/workbench-core/wasm-source.ts';
import { createFakeWorkers, type FakeWorker } from '@digi/workbench-core/workers-fake.ts';

interface Request {
  type: string;
  id?: number;
  base?: string;
  restore?: boolean;
  set?: string;
}

const prepared = (restore: boolean): Omit<PreparedFlash, 'image'> & { image: ArrayBuffer } =>
  ({ ok: true, restore, device: 'dn2', os: '1.12', image: new Uint8Array([9, 9]).buffer }) as never;

/** The patch worker: ready after init, `prepare` answered with a canned image. */
const patchWorker = (seen: Request[]) => (message: unknown, worker: FakeWorker) => {
  const m = message as Request;
  seen.push(m);
  if (m.type === 'init') worker.reply({ type: 'ready' });
  else if (m.type === 'prepare') worker.reply({ type: 'prepared', id: m.id, prepared: prepared(!!m.restore) });
};

test('prepare starts one patch worker with the base URL, then asks it for the image', async () => {
  const seen: Request[] = [];
  const workers = createFakeWorkers({ patch: patchWorker(seen) });
  const env = createWebFirmwareEnv({ workers, base: '/site/', wasm: memoryWasmSource() });
  const a = await env.prepare(new Uint8Array([1]), false, 'device');
  const b = await env.prepare(new Uint8Array([2]), true);
  assert.deepEqual([...a.image], [9, 9]);
  assert.equal(a.restore, false);
  assert.equal(b.restore, true);
  assert.equal(workers.made.length, 1, 'one worker for both');
  assert.deepEqual(
    seen.map((m) => m.type),
    ['init', 'prepare', 'prepare'],
  );
  assert.equal(seen[0]!.base, '/site/');
  assert.equal(seen[1]!.set, 'device');
});

test('a worker that fails to start rejects the call, and the next call starts a new worker', async () => {
  let first = true;
  const workers = createFakeWorkers({
    patch: (message, worker) => {
      if ((message as Request).type !== 'init') return;
      if (first) {
        first = false;
        worker.reply({ type: 'error', error: 'patch tool files are missing from this site' });
      } else worker.reply({ type: 'ready' });
    },
  });
  const env = createWebFirmwareEnv({ workers, wasm: memoryWasmSource() });
  await assert.rejects(env.prepare(new Uint8Array(1), false), /patch tool files are missing/);
  // the failed worker is gone; the second call makes another
  const second = env.prepare(new Uint8Array(1), false);
  second.catch(() => {});
  await new Promise((r) => setTimeout(r, 5));
  assert.equal(workers.made.length, 2);
});

test('the sender comes from the wasm source once, through a stub kit', async () => {
  const wasm = memoryWasmSource({ 'patchkit.wasm': new Uint8Array([1]) });
  let made = 0;
  const kit = {
    sender: (total: number) => ({ next: () => ({ kind: 'done' as const }), fail() {}, sent: () => total, free() {} }),
  } as unknown as PatchKitLike;
  const env = createWebFirmwareEnv({
    workers: createFakeWorkers(),
    wasm,
    createKit: async () => {
      made++;
      return kit;
    },
  });
  assert.equal((await env.sender(7)).sent(), 7);
  await env.sender(3);
  assert.equal(made, 1);
  assert.deepEqual(wasm.asked, ['patchkit.wasm']);
});
