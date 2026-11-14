// browserRuntime (packages/workbench-core/src/runtime/runtime.ts) over a fake worker and a stub wasm source: the messages it posts, the replies it waits for,
// and the updates it passes on (only those of the current load).
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { browserRuntime, type RuntimeSnapshot, type RuntimeUpdate } from '@digi/workbench-core/runtime/runtime.ts';
import { EMPTY_WASM, memoryWasmSource } from '@digi/workbench-core/wasm-source.ts';
import { createFakeWorkers, type FakeWorker } from '@digi/workbench-core/workers-fake.ts';

const snapshot = (icount: number): RuntimeSnapshot => ({
  status: {
    device: 'dn2',
    version: '1.12',
    icount,
    pc: 0,
    ready: true,
    phase: 'main',
    error: null,
    frame_revision: 0,
    frame_source: null,
    main_ui_reached: true,
    filesystem_verified: null,
    input_ready: true,
    input_pending: 0,
    input_irqs: 0,
  },
});

interface Posted {
  type: string;
  id?: string;
  generation?: number;
}

/** A worker that answers each request that carries an id with `answer(message)`. */
const answering = (answer: (m: Posted) => unknown) => (message: unknown, worker: FakeWorker) => {
  const m = message as Posted;
  if (m.id) worker.reply({ reply: m.id, value: answer(m) });
};

test('a load posts the file to the runtime worker and resolves with its reply; the worker is made once, of kind runtime', async () => {
  const workers = createFakeWorkers({ runtime: answering(() => snapshot(1)) });
  const wasm = memoryWasmSource();
  const runtime = browserRuntime(() => {}, { workers, wasm });
  const got = await runtime.load(new Uint8Array([1, 2, 3]), 'a.syx');
  assert.equal(got.status.icount, 1);
  assert.deepEqual(
    workers.made.map((w) => w.kind),
    ['runtime'],
  );
  const posted = workers.made[0]!.posted.map((p) => p.message as Posted);
  assert.equal(posted[0]!.type, 'load');
  assert.equal(posted[0]!.generation, 1);
  assert.deepEqual(wasm.asked, [], 'a load needs no wasm from the page');
});

test('updates of an older load are dropped, those of the current one reach the page', async () => {
  const workers = createFakeWorkers({ runtime: answering(() => snapshot(1)) });
  const updates: RuntimeUpdate[] = [];
  const runtime = browserRuntime((u) => updates.push(u), { workers, wasm: memoryWasmSource() });
  await runtime.load(new Uint8Array(4), 'a.syx');
  await runtime.load(new Uint8Array(4), 'b.syx'); // generation 2
  const worker = workers.made[0]!;
  worker.reply({ type: 'snapshot', snapshot: snapshot(10), generation: 1 });
  worker.reply({ type: 'snapshot', snapshot: snapshot(20), generation: 2 });
  assert.deepEqual(
    updates.map((u) => (u.type === 'snapshot' ? u.snapshot.status.icount : -1)),
    [20],
  );
});

test('a worker error reaches the request as a rejection', async () => {
  const workers = createFakeWorkers({
    runtime: (message, worker) => {
      const m = message as Posted;
      if (m.id) worker.reply({ reply: m.id, error: 'not a Digitakt II image' });
    },
  });
  const runtime = browserRuntime(() => {}, { workers, wasm: memoryWasmSource() });
  await assert.rejects(runtime.load(new Uint8Array(1), 'bad.syx'), /not a Digitakt II image/);
});

test('pause, resume and stop are posted with the generation; dispose ends the worker', async () => {
  const workers = createFakeWorkers({ runtime: answering(() => snapshot(1)) });
  const runtime = browserRuntime(() => {}, { workers, wasm: memoryWasmSource() });
  await runtime.load(new Uint8Array(1), 'a.syx');
  runtime.pause();
  runtime.resume();
  runtime.resume(); // already running: nothing is posted
  await runtime.stop();
  const types = workers.made[0]!.posted.map((p) => (p.message as Posted).type);
  assert.deepEqual(types, ['load', 'pause', 'resume', 'stop']);
  runtime.dispose();
  assert.equal(workers.made[0]!.terminated, true);
});

test('a coupled load with a core that cannot split takes the single worker: the core comes from the wasm source, the audio port goes to the worker', async () => {
  const workers = createFakeWorkers({ runtime: answering(() => snapshot(2)) });
  const wasm = memoryWasmSource({ 'emulator-core-sharc.wasm': EMPTY_WASM });
  const runtime = browserRuntime(() => {}, { workers, wasm });
  const channel = new MessageChannel();
  const files = {
    syx: new Uint8Array(2),
    image: new Uint8Array(2),
    dsp: new Uint8Array(2),
    snapshot: new Uint8Array(2),
  };
  const got = await runtime.loadCoupled!(files, channel.port1);
  channel.port1.close();
  channel.port2.close();
  assert.equal(got.status.icount, 2);
  const types = workers.made[0]!.posted.map((p) => (p.message as Posted).type);
  assert.deepEqual(types, ['audio-port', 'load-coupled']);
  assert.deepEqual(
    workers.made.map((w) => w.kind),
    ['runtime'],
    'no DSP worker without cross-origin isolation',
  );
});
