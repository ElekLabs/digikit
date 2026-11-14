// The desktop runtime (apps/desktop/ui/src/native-runtime.ts) over a FakeHost: when it is offered, and the calls of a load.
import assert from 'node:assert/strict';
import { test, vi } from 'vitest';
import type { RuntimeSnapshot, RuntimeUpdate } from '../../../web/src/runtime/runtime.ts';
import { createFakeHost } from '../src/host/fake-host.ts';
import { createNativeRuntime } from '../src/native-runtime.ts';

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

test('outside the app (a plain browser) there is no native runtime, and nothing is asked of the emulator', async () => {
  const host = createFakeHost({ 'emu.inApp': false });
  assert.equal(await createNativeRuntime(host, () => {}), undefined);
  assert.deepEqual(host.commands(), ['emu.inApp']);
});

test('inside the app a startup firmware shows its snapshot at once, and the app without one starts empty', async () => {
  const updates: RuntimeUpdate[] = [];
  const host = createFakeHost({ 'emu.startup': snapshot(5), 'emu.step': snapshot(6) });
  const runtime = await createNativeRuntime(host, (u) => updates.push(u));
  assert.ok(runtime);
  assert.equal(updates[0]?.type === 'snapshot' && updates[0].snapshot.status.icount, 5);
  runtime.dispose();
  const empty = createFakeHost({
    'emu.startup': () => {
      throw new Error('No startup firmware selected');
    },
  });
  const updates2: RuntimeUpdate[] = [];
  const idle = await createNativeRuntime(empty, (u) => updates2.push(u));
  assert.ok(idle, 'the runtime is there; it has no firmware yet');
  assert.deepEqual(updates2, [], 'no startup firmware is no error');
  idle.dispose();
});

test('a load stages the file, loads it through the controller with the session token, and shows the new snapshot', async () => {
  const host = createFakeHost({
    'emu.startup': snapshot(1),
    'emu.stageSyx': '/tmp/staged.syx',
    'rpc.call': () => ({}),
    'emu.step': snapshot(3),
  });
  const updates: RuntimeUpdate[] = [];
  const runtime = (await createNativeRuntime(host, (u) => updates.push(u)))!;
  const got = await runtime.load(new Uint8Array([1, 2]), 'a.syx', { runtime: true });
  assert.equal(got.status.icount, 1);
  const [staged] = host.argsOf('emu.stageSyx');
  assert.equal(staged![1], 'a.syx');
  const [load] = host.argsOf('rpc.call');
  assert.deepEqual(load, ['firmware.load', { syx: '/tmp/staged.syx', replace: true, runtime: true }, 2]);
  runtime.dispose();
  assert.equal(host.commands().at(-1), 'emu.stop');
});

test('a refusal with a code becomes an error with the code as data', async () => {
  const host = createFakeHost({
    'emu.startup': snapshot(1),
    'rpc.call': () => {
      throw new Error('cancelled (code 10)');
    },
  });
  const runtime = (await createNativeRuntime(host, () => {}))!;
  await assert.rejects(
    runtime.cancelJob!(),
    (e: Error & { code?: number }) => e.message === 'cancelled' && e.code === 10,
  );
  runtime.dispose();
});

// The lightweight audio status, and a load refused while a job runs (the controller's session stays the current one).
const status = (extra: object = {}) => ({ status: { ready: true, ...extra } }) as unknown as RuntimeSnapshot;

test('native audio health uses its lightweight endpoint and preserves the CF setup fallback', async () => {
  vi.useFakeTimers();
  try {
    const host = createFakeHost({ 'emu.startup': status({ ready: false }), 'emu.audioStatus': null });
    const runtime = (await createNativeRuntime(host, () => {}))!;
    assert.deepEqual(await runtime.nativeAudio!(), {
      sink_connected: false,
      playback_requested: false,
      flow_started: false,
      source_audio_seconds: 0,
      session_wall_seconds: 0,
      device_error: null,
      setup_needed: true,
    });
    assert.deepEqual(host.argsOf('emu.audioStatus'), [[1]]);
    runtime.dispose();
  } finally {
    vi.useRealTimers();
  }
});

test('native audio health has no session before startup firmware exists', async () => {
  vi.useFakeTimers();
  try {
    const host = createFakeHost({
      'emu.startup': () => {
        throw new Error('No startup firmware selected');
      },
    });
    const runtime = (await createNativeRuntime(host, () => {}))!;
    assert.equal(await runtime.nativeAudio!(), undefined);
    runtime.dispose();
  } finally {
    vi.useRealTimers();
  }
});

test('a busy firmware load preserves the current session, input and polling; retry uses a fresh token', async () => {
  vi.useFakeTimers();
  try {
    const updates: RuntimeUpdate[] = [];
    const steps: number[] = [];
    const inputs: (number | undefined)[] = [];
    let session = 1;
    let busy = true;
    const snapshot = () => status({ session });
    const host = createFakeHost({
      'emu.startup': snapshot,
      'emu.step': (id: number) => {
        assert.equal(id, session);
        steps.push(id);
        return snapshot();
      },
      'rpc.call': (method: string, _params: unknown, token?: number) => {
        if (method === 'firmware.load') {
          if (busy) throw new Error('a job is running; wait for it before loading firmware');
          session = token!;
        } else if (method === 'input.button') inputs.push(token);
        return {};
      },
    });
    const runtime = (await createNativeRuntime(host, (u) => updates.push(u)))!;
    await assert.rejects(runtime.loadPath!('test.syx'), /a job is running/);
    const last = updates.filter((u) => u.type === 'snapshot').at(-1) as { snapshot: { status: { session?: number } } };
    assert.equal(last.snapshot.status.session, 1);
    await runtime.press(1);
    await vi.runOnlyPendingTimersAsync();
    assert.deepEqual(inputs, [1]);
    assert.deepEqual(steps, [1], 'polling resumes on the accepted controller session');
    busy = false;
    await runtime.loadPath!('test.syx');
    assert.equal(session, 3, 'the refused request must not reuse its session token');
    await vi.runOnlyPendingTimersAsync();
    assert.deepEqual(steps, [1, 3]);
    runtime.dispose();
  } finally {
    vi.useRealTimers();
  }
});

test('a busy firmware load preserves an explicitly paused session', async () => {
  vi.useFakeTimers();
  try {
    const host = createFakeHost({
      'emu.startup': status(),
      'rpc.call': () => {
        throw new Error('a job is running; wait for it before loading firmware');
      },
    });
    const runtime = (await createNativeRuntime(host, () => {}))!;
    runtime.pause();
    await assert.rejects(runtime.loadPath!('test.syx'), /a job is running/);
    assert.equal(vi.getTimerCount(), 0, 'a refused load must not resume a paused emulator');
    runtime.resume();
    assert.equal(vi.getTimerCount(), 1);
    runtime.dispose();
  } finally {
    vi.useRealTimers();
  }
});

test('when a queued load is refused, recover the session accepted by the preceding load', async () => {
  vi.useFakeTimers();
  try {
    let session = 1;
    let acceptFirst!: () => void;
    const updates: RuntimeUpdate[] = [];
    const host = createFakeHost({
      'emu.startup': () => status({ session }),
      'rpc.call': (_method: string, _params: unknown, token?: number) => {
        if (token === 2)
          return new Promise((resolve) => {
            acceptFirst = () => {
              session = 2;
              resolve({});
            };
          });
        throw new Error('a job is running; wait for it before loading firmware');
      },
    });
    const runtime = (await createNativeRuntime(host, (u) => updates.push(u)))!;
    const first = runtime.loadPath!('first.syx');
    const second = runtime.loadPath!('second.syx');
    const results = Promise.allSettled([first, second]);
    await vi.advanceTimersByTimeAsync(0);
    acceptFirst();
    assert.deepEqual(
      (await results).map((r) => r.status),
      ['fulfilled', 'rejected'],
    );
    const last = updates.filter((u) => u.type === 'snapshot').at(-1) as { snapshot: { status: { session?: number } } };
    assert.equal(last.snapshot.status.session, 2);
    assert.equal(vi.getTimerCount(), 1, 'the accepted session has a running pump');
    runtime.dispose();
  } finally {
    vi.useRealTimers();
  }
});
