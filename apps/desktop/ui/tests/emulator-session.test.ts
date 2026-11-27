import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import type { EmuLaunch } from '@digi/workbench-core/host/host.ts';
import { createRoot } from 'solid-js';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { createEmulatorSession } from '../src/emulator/session';

const snapshot = { status: { ready: true, icount: 5 } };
const launch = { syx: null, card: null, runtime: null, loaded: true };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

function mountSession(host: ReturnType<typeof createFakeHost>) {
  const actions = {
    update: vi.fn(),
    applyLaunch: vi.fn(),
    pollSource: vi.fn(),
    pollAudio: vi.fn(),
    pollHealth: vi.fn(),
  };
  const owner = createRoot((dispose) => ({ dispose, ...createEmulatorSession({ host, ...actions }) }));
  return { ...owner, ...actions };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

test('disposing during native startup releases a late runtime without publishing or polling', async () => {
  const startup = deferred<typeof snapshot>();
  const host = createFakeHost({ 'emu.startup': () => startup.promise, 'emu.step': snapshot, 'emu.launch': launch });
  const owner = mountSession(host);
  expect(host.argsOf('emu.startup')).toHaveLength(1);

  owner.dispose();
  const schedulePump = vi.spyOn(globalThis, 'setTimeout');
  startup.resolve(snapshot);
  await vi.advanceTimersByTimeAsync(0);

  expect(owner.update).not.toHaveBeenCalled();
  expect(owner.runtime()).toBeUndefined();
  expect(host.argsOf('emu.stop')).toHaveLength(1);
  expect(host.argsOf('emu.launch')).toHaveLength(0);
  expect(schedulePump).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

test('a live session publishes startup and stops all of its polling when disposed', async () => {
  const step = deferred<typeof snapshot>();
  const buildInfo = { pgo: 'missing' };
  const host = createFakeHost({
    'emu.startup': snapshot,
    'emu.step': () => step.promise,
    'emu.launch': launch,
    'emu.buildInfo': buildInfo,
  });
  const owner = mountSession(host);

  await vi.advanceTimersByTimeAsync(1000);
  expect(owner.runtime()).toBeDefined();
  expect(owner.update).toHaveBeenCalledExactlyOnceWith({ type: 'snapshot', snapshot });
  expect(owner.applyLaunch).toHaveBeenCalledExactlyOnceWith(launch);
  expect(owner.buildInfo()).toEqual(buildInfo);
  expect(host.argsOf('emu.step')).toHaveLength(1);
  for (const poll of [owner.pollSource, owner.pollAudio, owner.pollHealth]) expect(poll).toHaveBeenCalledTimes(1);

  owner.dispose();
  expect(owner.runtime()).toBeUndefined();
  expect(vi.getTimerCount()).toBe(0);
  step.resolve(snapshot);
  await vi.advanceTimersByTimeAsync(2000);

  expect(owner.update).toHaveBeenCalledTimes(1);
  expect(host.argsOf('emu.step')).toHaveLength(1);
  expect(host.argsOf('emu.stop')).toHaveLength(1);
  for (const poll of [owner.pollSource, owner.pollAudio, owner.pollHealth]) expect(poll).toHaveBeenCalledTimes(1);
});

test('a launch file and build details returned after disposal are ignored', async () => {
  const pendingLaunch = deferred<EmuLaunch>();
  const pendingBuild = deferred<unknown>();
  const host = createFakeHost({
    'emu.startup': () => {
      throw new Error('No startup firmware selected');
    },
    'emu.launch': () => pendingLaunch.promise,
    'emu.buildInfo': () => pendingBuild.promise,
    'emu.stop': () => Promise.reject(new Error('Controller already closed')),
  });
  const owner = mountSession(host);
  await vi.advanceTimersByTimeAsync(0);
  expect(owner.runtime()).toBeDefined();
  expect(host.argsOf('emu.launch')).toHaveLength(1);

  owner.dispose();
  pendingLaunch.resolve({ ...launch, syx: '/tmp/launch.syx', loaded: false });
  pendingBuild.resolve({ pgo: 'missing' });
  await vi.advanceTimersByTimeAsync(1000);

  expect(owner.applyLaunch).not.toHaveBeenCalled();
  expect(owner.buildInfo()).toBeUndefined();
  expect(host.argsOf('emu.stop')).toHaveLength(1);
  expect(vi.getTimerCount()).toBe(0);
});

test('a launch failure is shown while the window is live', async () => {
  const host = createFakeHost({
    'emu.startup': () => {
      throw new Error('No startup firmware selected');
    },
    'emu.buildInfo': undefined,
    'emu.launch': () => {
      throw new Error('Launch settings unavailable');
    },
  });
  const owner = mountSession(host);
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.update).toHaveBeenCalledExactlyOnceWith({ type: 'error', error: 'Launch settings unavailable' });
    expect(owner.applyLaunch).not.toHaveBeenCalled();
  } finally {
    owner.dispose();
  }
});
