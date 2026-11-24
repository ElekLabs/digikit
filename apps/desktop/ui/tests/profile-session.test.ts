import { DkSession } from '@digi/dk-session';
import { NoRuntimeError } from '@digi/workbench-core/modules/backend.ts';
import type { RunOutcome } from '@digi/workbench-core/profile/calibration-run.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import { createFakeTransferApp, silentTransport } from '@digi/workbench-core/transfer/fake-app.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import { createRoot, createSignal, onCleanup } from 'solid-js';
import { afterEach, expect, test, vi } from 'vitest';
import type { Calibrator } from '../src/calibration-backend';
import { createCalibrationHub } from '../src/calibration-hub';
import { createCalibrationState } from '../src/calibration-state';
import type { ProfileReading, Recorder } from '../src/profile/recorder';
import { createProfileSession } from '../src/profile/session';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((finish, fail) => {
    resolve = finish;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function reading(elapsed: number, running = true): ProfileReading {
  return {
    noDebug: false,
    port: 'fake-device-0.0',
    emulator: true,
    traceSupported: true,
    observedAt: elapsed * 1000,
    modules: [],
    snap: { running, hz: 1000, elapsedTicks: BigInt(elapsed * 1000), spans: [] },
  };
}

function fakeRecorder() {
  return {
    read: vi.fn<Recorder['read']>().mockResolvedValue(reading(1, false)),
    start: vi.fn<Recorder['start']>().mockResolvedValue(undefined),
    stop: vi.fn<Recorder['stop']>().mockResolvedValue(undefined),
    reset: vi.fn<Recorder['reset']>().mockResolvedValue(undefined),
    releaseDsp: vi.fn<Recorder['releaseDsp']>().mockResolvedValue(undefined),
    exportFile: vi.fn<Recorder['exportFile']>().mockResolvedValue({ text: '{}\n', name: 'capture.dkprof.json' }),
  };
}

function inputs() {
  const [visible, setVisible] = createSignal(true);
  const [collectSpans, setCollectSpans] = createSignal(false);
  const [liveDsp, setLiveDsp] = createSignal(false);
  const [cardRefresh, refresh] = createSignal(0);
  const [emulatorRuntime, setRuntime] = createSignal<'yes' | 'no' | 'unknown'>('yes');
  const session = new DkSession(silentTransport());
  const source = {
    target: { kind: 'emulator' },
    check: vi.fn(),
    beforeLeave: vi.fn<TargetBinding['beforeLeave']>(() => () => true),
    session: vi.fn<TargetBinding['session']>().mockResolvedValue(session),
  } satisfies TargetBinding;
  return {
    setVisible,
    setCollectSpans,
    setLiveDsp,
    refresh: () => refresh((v) => v + 1),
    setRuntime,
    deps: {
      source,
      visible,
      collectSpans,
      liveDsp,
      cardRefresh,
      emulatorRuntime,
      checkRuntime: () => {
        if (emulatorRuntime() === 'no') throw new NoRuntimeError('Boot with runtime');
      },
      recorder: fakeRecorder(),
      calibrating: () => false,
      noteReply: vi.fn<Parameters<typeof createProfileSession>[0]['noteReply']>().mockResolvedValue(undefined),
      installDebug: vi.fn().mockResolvedValue(undefined),
      save: vi.fn().mockResolvedValue(undefined),
    },
  };
}

function open(deps: Parameters<typeof createProfileSession>[0]) {
  return createRoot((dispose) => ({ dispose, owner: createProfileSession(deps) }));
}

afterEach(() => vi.useRealTimers());

test('a visible Profile reads full details, the hidden drawer reads spans, and polling stops when both hide', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const { read } = f.deps.recorder;
  read.mockResolvedValueOnce(reading(1)).mockResolvedValueOnce(reading(2)).mockResolvedValueOnce(reading(3));
  const { owner, dispose } = open(f.deps);
  try {
    await vi.advanceTimersByTimeAsync(999);
    expect(read.mock.calls).toEqual([['full', false]]);
    await vi.advanceTimersByTimeAsync(1);
    expect(read.mock.calls).toEqual([
      ['full', false],
      ['full', false],
    ]);
    expect(owner.columns()).toHaveLength(1);
    expect(owner.recording.intervals()).toMatchObject([{ start: 1000, end: 2000 }]);

    f.setLiveDsp(true);
    f.setCollectSpans(true);
    f.setVisible(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(read.mock.lastCall).toEqual(['spans', true]);
    expect(owner.recording.intervals()).toHaveLength(2);

    f.setCollectSpans(false);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(read).toHaveBeenCalledTimes(3);
    expect(owner.recording.intervals()).toHaveLength(2);

    f.setVisible(true);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(read).toHaveBeenCalledTimes(4);
    expect(read.mock.lastCall).toEqual(['full', true]);
    expect(owner.recording.running()).toBe(false);
  } finally {
    dispose();
  }
});

test('a runtime restart clears history and a card refresh retries an earlier failed read', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const { read } = f.deps.recorder;
  read.mockResolvedValueOnce(reading(1)).mockResolvedValueOnce(reading(2)).mockRejectedValueOnce(new Error('No reply'));
  const { owner, dispose } = open(f.deps);
  try {
    await vi.advanceTimersByTimeAsync(1000);
    expect(owner.columns()).toHaveLength(1);
    expect(owner.recording.intervals()).toHaveLength(1);

    f.setRuntime('no');
    expect(owner.noRuntime()).toBe(true);
    expect(owner.error()).toContain('Boot with runtime');
    expect(owner.reading()).toBeUndefined();
    expect(owner.columns()).toEqual([]);
    expect(owner.recording.intervals()).toEqual([]);

    f.setRuntime('yes');
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.error()).toContain('No reply');
    expect(owner.reading()).toBeUndefined();
    f.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledTimes(4);
    expect(owner.noRuntime()).toBe(false);
    expect(owner.error()).toBe('');
  } finally {
    dispose();
  }
});

test('leaving a target discards an in-flight reading and disables polling and commands', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const app = createFakeTransferApp();
  const stale = deferred<ProfileReading>();
  const { recorder, noteReply } = f.deps;
  recorder.read.mockResolvedValueOnce(reading(1)).mockReturnValueOnce(stale.promise);
  const { owner, dispose } = open({ ...f.deps, source: app.target.bindSession() });
  try {
    await vi.advanceTimersByTimeAsync(1000);
    const previous = owner.reading();
    expect(previous).toEqual(reading(1));
    expect(recorder.read).toHaveBeenCalledTimes(2);
    expect(noteReply).toHaveBeenCalledTimes(1);

    await app.target.selectTarget({ kind: 'midi', port: 'replacement' });
    stale.resolve(reading(2));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(owner.reading()).toBe(previous);
    expect(owner.columns()).toEqual([]);
    expect(owner.recording.intervals()).toEqual([]);
    expect(owner.recording.disabled()).toBe(true);
    expect(noteReply).toHaveBeenCalledTimes(1);
    expect(noteReply.mock.calls[0]![1]()).toBe(false);
    expect(recorder.read).toHaveBeenCalledTimes(2);
    await owner.recording.reset();
    expect(recorder.reset).not.toHaveBeenCalled();
  } finally {
    dispose();
  }
  expect(recorder.releaseDsp).toHaveBeenCalledTimes(1);
});

test('disposal invalidates a pending connection reply lookup', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const pendingSession = deferred<DkSession>();
  const { source, noteReply, recorder } = f.deps;
  source.session.mockReturnValueOnce(pendingSession.promise);
  const { dispose } = open(f.deps);
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(source.session).toHaveBeenCalledTimes(1);
  } finally {
    dispose();
  }
  pendingSession.resolve(new DkSession(silentTransport()));
  await vi.advanceTimersByTimeAsync(0);
  expect(noteReply).not.toHaveBeenCalled();
  expect(recorder.releaseDsp).toHaveBeenCalledTimes(1);
});

test('target leave and disposal share one DSP release after a recording action settles', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const { recorder, source } = f.deps;
  const started = deferred<void>();
  recorder.start.mockReturnValueOnce(started.promise);
  const { owner, dispose } = open(f.deps);
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.recording.includeSharc()).toBe(false);
    owner.recording.setIncludeDsp(true);
    owner.recording.setIncludeSharc(true);
    owner.recording.toggle();
    await vi.advanceTimersByTimeAsync(0);
    expect(recorder.start).toHaveBeenCalledWith(true, true);
    expect(owner.recording.disabled()).toBe(true);
    const leaving = source.beforeLeave.mock.calls[0]![0]();
    dispose();
    expect(recorder.releaseDsp).not.toHaveBeenCalled();
    started.reject(new Error('Old target rejected the start'));
    await leaving;
    expect(recorder.releaseDsp).toHaveBeenCalledTimes(1);
    expect(recorder.read).toHaveBeenCalledTimes(1);
    expect(owner.error()).toBe('');
  } finally {
    dispose();
  }
});

test('target leave waits for an in-flight export to save before releasing DSP', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const capture = deferred<Awaited<ReturnType<Recorder['exportFile']>>>();
  const saving = deferred<undefined>();
  const { recorder, save, source } = f.deps;
  recorder.exportFile.mockReturnValueOnce(capture.promise);
  save.mockReturnValueOnce(saving.promise);
  const { owner, dispose } = open(f.deps);
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.recording.canSave()).toBe(true);
    const exporting = owner.recording.save();
    await vi.advanceTimersByTimeAsync(0);
    expect(recorder.exportFile).toHaveBeenCalledTimes(1);
    const leaving = source.beforeLeave.mock.calls[0]![0]();
    await owner.recording.save();
    expect(recorder.exportFile).toHaveBeenCalledTimes(1);
    expect(recorder.releaseDsp).not.toHaveBeenCalled();

    const file = { text: '{}\n', name: 'old.dkprof.json' };
    capture.resolve(file);
    await vi.advanceTimersByTimeAsync(0);
    expect(save).toHaveBeenCalledExactlyOnceWith(file);
    expect(recorder.releaseDsp).not.toHaveBeenCalled();

    saving.resolve(undefined);
    await exporting;
    await leaving;
    expect(recorder.releaseDsp).toHaveBeenCalledTimes(1);
  } finally {
    dispose();
  }
});

test('Calibration takes over an active Profile read, blocks Profile commands, and then hands reads back', async () => {
  vi.useFakeTimers();
  const f = inputs();
  const { recorder, noteReply, installDebug, save } = f.deps;
  const stale = deferred<ProfileReading>();
  recorder.read.mockReturnValueOnce(stale.promise).mockResolvedValueOnce(reading(5));
  const run = deferred<RunOutcome>();
  const calibrator: Calibrator = {
    profiles: () => [
      {
        name: 'load',
        description: '',
        seconds: 1,
        profile: { name: 'load', description: '', warmup_s: 0, capture_s: 1, repeats: 1, modules: [], cases: [] },
      },
    ],
    run: () => run.promise,
    fit: async () => ({}),
  };
  const { owner, calibration, dispose } = createRoot((dispose) => {
    const hub = createCalibrationHub(memoryKeyValue());
    const owner = createProfileSession({ ...f.deps, calibrating: hub.calibrating });
    const calibration = createCalibrationState({
      calibrator,
      emulator: () => true,
      blocked: () => false,
      onRunning: hub.setCalibrating,
    });
    onCleanup(calibration.dispose);
    return { owner, calibration, dispose };
  });

  try {
    expect(recorder.read).toHaveBeenCalledTimes(1);
    const calibrating = calibration.run();
    expect(owner.recording.status()).toBe('Calibration in progress');
    expect(owner.recording.disabled()).toBe(true);
    stale.resolve(reading(1));
    f.refresh();
    owner.recording.setIncludeDsp(true);
    owner.recording.toggle();
    await Promise.all([owner.recording.reset(), owner.recording.save(), owner.addDebug()]);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(owner.reading()).toBeUndefined();
    expect(noteReply).not.toHaveBeenCalled();
    expect(recorder.read).toHaveBeenCalledTimes(1);
    expect(recorder.start).not.toHaveBeenCalled();
    expect(recorder.stop).not.toHaveBeenCalled();
    expect(recorder.reset).not.toHaveBeenCalled();
    expect(recorder.exportFile).not.toHaveBeenCalled();
    expect(installDebug).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    expect(owner.recording.includeDsp()).toBe(false);

    run.resolve({ profile: 'load', emulator: true, captures: [], skipped: [], failed: [], stopped: false });
    await calibrating;
    await vi.advanceTimersByTimeAsync(0);
    expect(recorder.read).toHaveBeenCalledTimes(2);
    expect(owner.reading()).toEqual(reading(5));
    expect(owner.recording.disabled()).toBe(false);
  } finally {
    dispose();
  }
});
