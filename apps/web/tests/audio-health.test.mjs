import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import { hostMetrics, pcmMetrics } from '../src/runtime/runtime-metrics.ts';
import ts from 'typescript';

function processor() {
  let Processor;
  const reports = [];
  const context = {
    sampleRate: 48000,
    Float32Array,
    AudioWorkletProcessor: class {
      constructor() {
        this.port = { postMessage: (value) => reports.push(value) };
      }
    },
    registerProcessor: (_name, ctor) => {
      Processor = ctor;
    },
  };
  vm.runInNewContext(fs.readFileSync(new URL('../public/pcm-worklet.js', import.meta.url), 'utf8'), context);
  const p = new Processor();
  const send = (data) => p.port.onmessage({ data });
  const render = (n) => {
    const left = new Float32Array(n),
      right = new Float32Array(n);
    assert.equal(p.process([], [[left, right]]), true);
    return [Array.from(left), Array.from(right)];
  };
  return { p, send, render, reports };
}

function workerHarness({ ordinaryHasCoupling = false } = {}) {
  let result = {};
  let icount = 0;
  const buttons = [];
  const timers = [];
  const replies = new Map();
  const memory = new WebAssembly.Memory({ initial: 1 });
  const writeResult = (value) => {
    result = value;
    const bytes = new TextEncoder().encode(JSON.stringify(value));
    new Uint8Array(memory.buffer).set(bytes, 0);
    return 0;
  };
  const abi = {
    memory,
    digi_result_ptr: () => 0,
    digi_result_len: () => new TextEncoder().encode(JSON.stringify(result)).length,
    digi_stop: () => writeResult({}),
    digi_alloc: () => 8,
    digi_dealloc: () => {},
    digi_load: () => writeResult({ snapshot: { status: { icount } } }),
    digi_step: () => {
      icount += 100;
      return writeResult({ snapshot: { status: { icount } } });
    },
    digi_button: (code, down) => {
      buttons.push([code, down]);
      return writeResult({});
    },
  };
  if (ordinaryHasCoupling) abi.digi_load_coupled = () => writeResult({ snapshot: { status: { icount } } });
  const source = fs
    .readFileSync(new URL('../src/runtime/runtime-worker.ts', import.meta.url), 'utf8')
    .replace("import { LinkClient } from '../sab-link.ts';", 'class LinkClient {}')
    .replace("import.meta.env.BASE_URL.replace(/\\/?$/, '/')", "'/'")
    .replace(
      "import { hostMetrics, pcmMetrics } from './runtime-metrics';",
      'const hostMetrics = () => ({ reset() {}, loaded() {}, step() {}, report() { return {}; } }); const pcmMetrics = () => ({});',
    );
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const context = {
    WebAssembly: {
      Memory: WebAssembly.Memory,
      instantiateStreaming: async (response) => {
        await response;
        return { instance: { exports: abi } };
      },
    },
    fetch: async (url) => {
      if (String(url).includes('sharc')) throw new Error('missing sharc core');
      return {};
    },
    TextDecoder,
    TextEncoder,
    Uint8Array,
    Float32Array,
    performance: { now: () => 0 },
    setTimeout: (fn) => {
      timers.push(fn);
      return timers.length;
    },
    // The pump yields through a MessageChannel: each port2 post queues one port1 task, like a timer.
    MessageChannel: class {
      constructor() {
        const port1 = {};
        this.port1 = port1;
        this.port2 = {
          postMessage: () => {
            timers.push(() => port1.onmessage?.({}));
          },
        };
      }
    },
    postMessage: (message) => {
      if (message.reply) replies.get(message.reply)?.(message);
    },
    self: {},
    console,
  };
  vm.runInNewContext(js, context);
  const send = (data) =>
    new Promise((resolve, reject) => {
      replies.set(data.id, (message) => {
        replies.delete(data.id);
        if (message.error) reject(new Error(message.error));
        else resolve(message.value);
      });
      context.self.onmessage({ data });
    });
  return {
    send,
    buttons,
    runTimer: async () => {
      const timer = timers.shift();
      if (timer) await timer();
    },
  };
}

function nativeRuntimeHarness(invoke) {
  const original = fs.readFileSync(new URL('../src/runtime/runtime.ts', import.meta.url), 'utf8');
  const source = (
    'const hostMetrics = () => ({ reset() {}, loaded() {}, step() {}, invalidatePumpGap() {}, report() { return {}; } });\n' +
    original.slice(original.indexOf('export async function nativeRuntime'))
  )
    .replace("await import('@tauri-apps/api/core')", 'await fakeTauri()')
    .replaceAll('export ', '')
    .concat('\nglobalThis.nativeRuntime = nativeRuntime;');
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const timers = new Map();
  let timerId = 0;
  const context = {
    fakeTauri: async () => ({ isTauri: () => true, invoke }),
    performance: { now: () => 0 },
    window: {
      setTimeout: (fn) => {
        timers.set(++timerId, fn);
        return timerId;
      },
      clearTimeout(id) {
        timers.delete(id);
      },
    },
    console,
  };
  vm.runInNewContext(js, context);
  return {
    runtime: (update = () => {}) => context.nativeRuntime(update),
    timers,
    async runTimer() {
      const next = timers.entries().next().value;
      if (next) {
        timers.delete(next[0]);
        next[1]();
        await new Promise((resolve) => setImmediate(resolve));
      }
    },
  };
}

test('PCM rate is emitted stereo duration over this session wall time', () => {
  assert.deepEqual(pcmMetrics(96000, 2000), {
    pcm_produced_seconds: 1,
    production_elapsed_wall_seconds: 2,
    pcm_seconds_per_wall_second: 0.5,
  });
  assert.equal(pcmMetrics(0, 0).pcm_seconds_per_wall_second, 0);
});

test('host response histogram is bounded, resettable and exported by value', () => {
  const m = hostMetrics('wasm_abi');
  m.reset();
  const snapshot = { status: { ready: false } };
  for (const n of [0, 1, 2, 5, 10, 20, 50, 100, 101]) m.step(0, n, snapshot);
  const report = m.report();
  assert.deepEqual(report.step_response_wall_ms_buckets.counts, [2, 1, 1, 1, 1, 1, 1, 1]);
  assert.equal(report.chunks, 9);
  m.reset();
  assert.deepEqual(m.report().step_response_wall_ms_buckets.counts, Array(8).fill(0));
  assert.equal(report.step_response_wall_ms_buckets.counts[0], 2);
  assert.equal('measured_pump_scheduling' in report, false);
});

test('desktop pump gaps exclude input queued while a step response is in flight', () => {
  const m = hostMetrics('native_ipc');
  m.reset();
  const snapshot = { status: { ready: false } };
  m.step(10, 14, snapshot);
  m.invalidatePumpGap(); // input queues while the preceding emu_step is in flight
  m.step(17, 22, snapshot); // that response must not bridge to post-input work
  m.invalidatePumpGap(); // input completes before the following pump iteration
  m.step(100, 110, snapshot);
  m.step(115, 120, snapshot);
  const scheduling = m.report().measured_pump_scheduling;
  assert.equal(scheduling.gaps, 1);
  assert.equal(scheduling.gap_wall_ms, 5);
  assert.deepEqual(scheduling.gap_wall_ms_buckets.counts, [0, 0, 1, 0, 0, 0, 0, 0]);
});

test('native audio health uses its lightweight endpoint and preserves CF setup fallback', async () => {
  const calls = [];
  const native = nativeRuntimeHarness(async (name, args) => {
    calls.push([name, args]);
    if (name === 'emu_startup') return { status: { ready: false } };
    if (name === 'emu_audio_status') return null;
    throw new Error(`unexpected ${name}`);
  });
  const runtime = await native.runtime();
  assert.deepEqual(JSON.parse(JSON.stringify(await runtime.nativeAudio())), {
    sink_connected: false,
    playback_requested: false,
    flow_started: false,
    source_audio_seconds: 0,
    session_wall_seconds: 0,
    device_error: null,
    setup_needed: true,
  });
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    ['emu_startup', null],
    ['emu_audio_status', { sessionId: 1 }],
  ]);
});

test('native audio health has no session before startup firmware exists', async () => {
  const native = nativeRuntimeHarness(async (name) => {
    assert.equal(name, 'emu_startup');
    throw new Error('No startup firmware selected');
  });
  const runtime = await native.runtime();
  assert.equal(await runtime.nativeAudio(), undefined);
});

test('a busy firmware load preserves the current session, input and polling; retry uses a fresh token', async () => {
  const updates = [];
  const steps = [];
  const inputs = [];
  let session = 1;
  let busy = true;
  const snapshot = () => ({ status: { ready: true, session } });
  const native = nativeRuntimeHarness(async (name, args) => {
    if (name === 'emu_startup') return snapshot();
    if (name === 'emu_step') {
      assert.equal(args.sessionId, session);
      steps.push(args.sessionId);
      return snapshot();
    }
    if (name === 'emu_call' && args.method === 'firmware.load') {
      if (busy) throw new Error('a job is running; wait for it before loading firmware');
      session = args.session;
      return {};
    }
    if (name === 'emu_call' && args.method === 'input.button') {
      inputs.push(args.session);
      return {};
    }
    throw new Error(`unexpected ${name}`);
  });
  const runtime = await native.runtime((u) => updates.push(u));
  await assert.rejects(runtime.loadPath('test.syx'), /a job is running/);
  assert.equal(updates.filter((u) => u.type === 'snapshot').at(-1).snapshot.status.session, 1);
  await runtime.press(1);
  await native.runTimer();
  assert.deepEqual(inputs, [1]);
  assert.deepEqual(steps, [1], 'polling resumes on the accepted controller session');
  busy = false;
  await runtime.loadPath('test.syx');
  assert.equal(session, 3, 'the refused request must not reuse its session token');
  await native.runTimer();
  assert.deepEqual(steps, [1, 3]);
});

test('a busy firmware load preserves an explicitly paused session', async () => {
  const native = nativeRuntimeHarness(async (name, args) => {
    if (name === 'emu_startup') return { status: { ready: true } };
    if (name === 'emu_call' && args.method === 'firmware.load')
      throw new Error('a job is running; wait for it before loading firmware');
    throw new Error(`unexpected ${name}`);
  });
  const runtime = await native.runtime();
  runtime.pause();
  await assert.rejects(runtime.loadPath('test.syx'), /a job is running/);
  assert.equal(native.timers.size, 0, 'a refused load must not resume a paused emulator');
  runtime.resume();
  assert.equal(native.timers.size, 1);
});

test('when a queued load is refused, recover the session accepted by the preceding load', async () => {
  let session = 1;
  let acceptFirst;
  const updates = [];
  const native = nativeRuntimeHarness(async (name, args) => {
    if (name === 'emu_startup') return { status: { ready: true, session } };
    if (name === 'emu_call' && args.method === 'firmware.load') {
      if (args.session === 2)
        return new Promise((resolve) => {
          acceptFirst = () => {
            session = 2;
            resolve({});
          };
        });
      throw new Error('a job is running; wait for it before loading firmware');
    }
    throw new Error(`unexpected ${name}`);
  });
  const runtime = await native.runtime((u) => updates.push(u));
  const first = runtime.loadPath('first.syx');
  const second = runtime.loadPath('second.syx');
  const results = Promise.allSettled([first, second]);
  await new Promise((resolve) => setImmediate(resolve));
  acceptFirst();
  assert.deepEqual(
    (await results).map((r) => r.status),
    ['fulfilled', 'rejected'],
  );
  assert.equal(updates.filter((u) => u.type === 'snapshot').at(-1).snapshot.status.session, 2);
  assert.equal(native.timers.size, 1, 'the accepted session has a running pump');
});

test('buffer threshold has a 0.25 s minimum; stereo output is faded in; initial silence is not underrun', () => {
  const { p, send, render } = processor();
  send({ type: 'config', startSeconds: 4 / 48000 });
  send(new Float32Array([1, 2, 3, 4]));
  assert.deepEqual(render(2), [
    [0, 0],
    [0, 0],
  ]);
  assert.equal(p.silence, 2);
  assert.equal(p.underrunFrames, 0);
  send(new Float32Array([5, 6, 7, 8]));
  assert.deepEqual(render(4), [
    [0, 0, 0, 0],
    [0, 0, 0, 0],
  ]);
  send(new Float32Array(2 * 12000));
  const [l, r] = render(4);
  const want = [
    [1, 3, 5, 7],
    [2, 4, 6, 8],
  ];
  [l, r].forEach((ch, c) => ch.forEach((v, k) => assert.ok(Math.abs(v - (want[c][k] * k) / 128) < 1e-5)));
  assert.equal(p.highWater, 12004);
  assert.equal(p.played, 4);
});

test('underrun duration includes the partial quantum and subsequent rebuffering', () => {
  const { p, send, render, reports } = processor();
  send({ type: 'config', startSeconds: 0 });
  const port = {};
  send({ port });
  port.onmessage({ data: new Float32Array(2 * 12000).fill(1) });
  for (let q = 0; q < 93; q += 1) render(128);
  render(128);
  assert.equal(p.underruns, 1);
  assert.equal(p.fades, 1);
  assert.equal(p.underrunFrames, 32);
  render(128);
  assert.equal(p.underrunFrames, 160);
  p.sinceReport = 12000;
  render(4);
  const report = reports.at(-1);
  assert.equal(report.underrunSeconds, 164 / 48000);
  assert.equal(report.fades, 1);
  assert.equal(report.highWaterSeconds, 12000 / 48000);
  assert.equal(report.portAttached, true);
  assert.equal(report.receivedPcm, true);
  assert.equal(report.renderedPcm, true);
  send({ type: 'flush' });
  assert.equal(p.queued, 0);
  assert.equal(p.playing, false);
});

test('a burst fades in from zero and fades out before the silence', () => {
  const { p, send, render } = processor();
  send({ type: 'config', startSeconds: 0 });
  send(new Float32Array(2 * 14400).fill(1));
  const out = [];
  for (let q = 0; q < 200 && p.underruns === 0; q += 1) out.push(...render(128)[0]);
  assert.equal(p.underruns, 1);
  assert.equal(out[0], 0);
  assert.ok(out.some((v) => v === 1));
  let last = out.length - 1;
  while (out[last] === 0) last -= 1;
  assert.equal(last, 14400 - 1);
  for (let k = last - 63; k < last; k += 1) assert.ok(out[k] > out[k + 1]);
});

test('missing SHARC exports report unavailable capability before any coupled load', async () => {
  const worker = workerHarness();
  assert.equal(await worker.send({ type: 'coupled-capable', id: 'capability', generation: 0 }), false);
  assert.deepEqual(worker.buttons, []);
});

test('a deferred tap releases after an ordinary CF step', async () => {
  const worker = workerHarness();
  await worker.send({ type: 'load', id: 'load', generation: 1, bytes: new Uint8Array([1]).buffer });
  await worker.send({ type: 'tap', id: 'tap', generation: 1, code: 25, hold: 100 });
  await worker.runTimer();
  assert.deepEqual(worker.buttons, [
    [25, 1],
    [25, 0],
  ]);
});
