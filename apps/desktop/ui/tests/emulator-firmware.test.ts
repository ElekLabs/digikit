import { DkError, fileHashHex, wallClock, type DkInfoFull, type Quick } from '@digi/dk-session';
import { CAP } from '@digi/dk-session/gen_proto';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import type { FirmwareEnv } from '@digi/workbench-core/firmware/env.ts';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import type { EmuLaunch, EmuSource } from '@digi/workbench-core/host/host.ts';
import { createJobs } from '@digi/workbench-core/jobs/jobs.ts';
import type { ReloadReport } from '@digi/workbench-core/runtime/reload.ts';
import type { FirmwareInfo, LoadOptions, RuntimeSnapshot } from '@digi/workbench-core/runtime/runtime.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import type { Target } from '@digi/workbench-core/transfer/target.ts';
import { createRoot, createSignal, onCleanup } from 'solid-js';
import { expect, onTestFinished, test, vi } from 'vitest';
import { DSP_KEY } from '../src/dsp-core';
import { createEmulatorActions } from '../src/emulator-actions';
import { createEmulatorFirmware } from '../src/emulator/firmware';
import type { Page } from '../src/page-core';

const device = registry.runtimeIds[0]!;
const launch: EmuLaunch = { loaded: false, syx: null, runtime: null, card: null };
const snapshot: RuntimeSnapshot = {
  status: {
    device,
    version: '1.0',
    ready: true,
    phase: 'running',
    icount: 8,
    pc: 0,
    error: null,
    frame_revision: 1,
    frame_source: 'guest',
    main_ui_reached: true,
    filesystem_verified: true,
    input_ready: true,
    input_pending: 0,
    input_irqs: 0,
  },
};

function file(path: string, modified_ms = 1): EmuSource {
  return { name: path.split('/').at(-1)!, path, bytes: 1024, modified_ms, device };
}

function mountFirmware(
  initial: Record<string, string> = {},
  initialTarget: Target = { kind: 'emulator' },
  systemPackage?: Parameters<typeof createEmulatorFirmware>[0]['systemPackage'],
) {
  const host = createFakeHost({ 'emu.setSource': (path: string) => file(path) });
  const storage = memoryKeyValue(initial);
  const lifetime = new AbortController();
  const runtime = {
    loadPath: vi.fn(async (_path: string, _options?: LoadOptions) => snapshot),
    firmwareInfo: vi.fn(async (): Promise<FirmwareInfo> => ({ runtime: { patched: true } })),
    reloadModules: vi.fn(async (): Promise<ReloadReport> => ({ path: 'none', ms: 4 })),
    restart: vi.fn(async () => snapshot),
    cancelJob: vi.fn(async () => {}),
    pause: vi.fn(),
    resume: vi.fn(),
  };
  const callbacks = {
    showSnapshot: vi.fn(),
    releaseHeldKeys: vi.fn(),
    setError: vi.fn(),
    setReloadNote: vi.fn(),
    watchSnapshotError: vi.fn(),
    resetHealth: vi.fn(),
    controllerBusy: () => false,
    setBrowseDevice: vi.fn(),
    clearInventory: vi.fn(),
    bumpCardRefresh: vi.fn(),
  };
  const logWriter = { logEvent: vi.fn(), logInput: vi.fn() };
  const owner = createRoot((dispose) => {
    onCleanup(() => lifetime.abort());
    const [page, setPage] = createSignal<Page>('run');
    const [target, setTarget] = createSignal<Target>(initialTarget);
    const [emulatorRuntime, setEmulatorRuntime] = createSignal<'unknown' | 'yes' | 'no'>('unknown');
    const jobs = createJobs();
    const defaults = {
      tickedFor: vi.fn((): string[] => []),
      install: vi.fn<NonNullable<FirmwareEnv['defaultModules']>['install']>(async () => ['already on card']),
    };
    const firmware = createEmulatorFirmware({
      emu: host.emu,
      storage,
      runtime: () => runtime,
      signal: lifetime.signal,
      jobs,
      target: { target, emulatorRuntime, setEmulatorRuntime, setEmulatorAvailable: vi.fn() },
      connection: { noteConnection: vi.fn() },
      defaults,
      systemPackage,
      logWriter,
      page,
      ...callbacks,
    });
    return { dispose, setPage, setTarget, jobs, defaults, firmware };
  });
  onTestFinished(owner.dispose);
  return { ...owner, callbacks, host, runtime, storage, logWriter };
}

test('a launch file uses its card and runtime choice and applies the one-time DSP flag over the saved preference', async () => {
  const owner = mountFirmware({ [DSP_KEY]: 'on', 'digikit.bootRuntime': 'off' }, { kind: 'midi', port: 'USB' });
  const { firmware, runtime, defaults, jobs, storage, callbacks } = owner;
  runtime.firmwareInfo.mockResolvedValueOnce({ runtime: null, dsp: { enabled: true, flag: false } });
  defaults.tickedFor.mockReturnValue(['test/module']);
  let installJob: string | undefined;
  let installStatus: ReturnType<typeof firmware.status>;
  defaults.install.mockImplementation(async () => {
    installJob = jobs.currentJob()?.label;
    installStatus = firmware.status();
    return ['already on card'];
  });

  await firmware.applyLaunch({ ...launch, syx: '/tmp/launch.syx', card: '/tmp/card.img', runtime: true });
  await vi.waitFor(() => expect(jobs.busy('emulator')).toBe(false));

  expect(runtime.loadPath).toHaveBeenCalledExactlyOnceWith('/tmp/launch.syx', {
    dsp: false,
    runtime: true,
    card: '/tmp/card.img',
  });
  expect(callbacks.releaseHeldKeys).toHaveBeenCalledTimes(1);
  expect(defaults.install).toHaveBeenCalledTimes(1);
  expect(installJob).toBe('Installing default modules');
  expect(installStatus).toBe(snapshot.status);
  expect(firmware.bootDsp()).toBe(false);
  expect(storage.get(DSP_KEY)).toBe('on');
  expect(storage.get('digikit.bootRuntime')).toBe('off');
  expect(storage.get('digikit.lastFirmware')).toBe('/tmp/launch.syx');

  firmware.chooseDsp(false);
  firmware.chooseRuntime(true);
  firmware.chooseLoadOnStart(true);
  expect(storage.get(DSP_KEY)).toBe('off');
  expect(storage.get('digikit.bootRuntime')).toBe('on');
  expect(storage.get('digikit.loadOnStart')).toBe('on');
});

test('disposal aborts an in-flight default install and ignores its late completion', async () => {
  const { callbacks, defaults, dispose, firmware, jobs, logWriter, runtime } = mountFirmware();
  let finishInstall!: (lines: string[]) => void;
  defaults.tickedFor.mockReturnValue(['test/module']);
  defaults.install.mockImplementation(() => new Promise<string[]>((resolve) => (finishInstall = resolve)));

  await firmware.bootPath('/tmp/defaults.syx', 'Loaded at');
  expect(jobs.currentJob()?.label).toBe('Installing default modules');
  const installSignal = defaults.install.mock.calls[0]?.[4]?.signal;
  expect(installSignal?.aborted).toBe(false);
  const notices = callbacks.setReloadNote.mock.calls.length;
  const entries = logWriter.logInput.mock.calls.length;

  dispose();
  expect(installSignal?.aborted).toBe(true);
  finishInstall(['late result']);
  await vi.waitFor(() => expect(jobs.busy('emulator')).toBe(false));

  expect(runtime.cancelJob).not.toHaveBeenCalled();
  expect(callbacks.setReloadNote).toHaveBeenCalledTimes(notices);
  expect(logWriter.logInput).toHaveBeenCalledTimes(entries);
  expect(callbacks.bumpCardRefresh).toHaveBeenCalledTimes(1);
});

test('with a hardware target at launch the last file waits for the Emulator page', async () => {
  const { firmware, runtime, setPage, setTarget } = mountFirmware(
    { 'digikit.loadOnStart': 'on', 'digikit.lastFirmware': '/tmp/last.syx' },
    { kind: 'midi', port: 'USB' },
  );
  await firmware.applyLaunch(launch);
  expect(runtime.loadPath).not.toHaveBeenCalled();

  setPage('transfer');
  setTarget({ kind: 'emulator' });
  expect(runtime.loadPath).not.toHaveBeenCalled();

  setPage('run');
  await vi.waitFor(() => expect(firmware.status()).toBe(snapshot.status));
  setPage('transfer');
  setPage('run');
  expect(firmware.source()?.path).toBe('/tmp/last.syx');
  expect(runtime.loadPath).toHaveBeenCalledTimes(1);
});

test('the watcher remembers a file change while disabled and reloads for the next changed timestamp', async () => {
  const { firmware, host, runtime, callbacks } = mountFirmware();
  const path = '/tmp/watch.syx';
  host.replies['emu.pickSyx'] = path;
  host.replies['emu.setSource'] = () => host.replies['emu.sourceInfo'];
  host.replies['emu.sourceInfo'] = file(path);
  await firmware.openFirmware();
  await firmware.pollSource();
  expect(runtime.loadPath).toHaveBeenCalledTimes(1);

  firmware.toggleWatching();
  host.replies['emu.sourceInfo'] = file(path, 2);
  await firmware.pollSource();
  firmware.toggleWatching();
  await firmware.pollSource();
  expect(runtime.loadPath).toHaveBeenCalledTimes(1);

  host.replies['emu.sourceInfo'] = file(path, 3);
  await firmware.pollSource();
  await firmware.pollSource();
  expect(runtime.loadPath).toHaveBeenCalledTimes(2);
  expect(firmware.source()?.modified_ms).toBe(3);
  expect(callbacks.setReloadNote).toHaveBeenLastCalledWith(expect.stringMatching(/^Reloaded after a change at /));
});

test('a refused boot keeps the native adapter’s restored session and the last successful path', async () => {
  const { firmware, runtime, callbacks, storage } = mountFirmware();
  await firmware.bootPath('/tmp/working.syx', 'Loaded at');
  runtime.loadPath.mockImplementationOnce(async () => {
    expect(firmware.status()).toBeUndefined();
    firmware.update({ type: 'snapshot', snapshot });
    throw new Error('controller busy');
  });

  await firmware.bootPath('/tmp/refused.syx', 'Loaded at');

  expect(firmware.status()).toBe(snapshot.status);
  expect(firmware.booting()).toBeUndefined();
  expect(storage.get('digikit.lastFirmware')).toBe('/tmp/working.syx');
  expect(callbacks.setError).toHaveBeenLastCalledWith('controller busy');
  expect(callbacks.bumpCardRefresh).toHaveBeenCalledTimes(1);
});

test('module reload clears the emulator inventory, resumes its state and signals a refresh', async () => {
  const { firmware, callbacks, setTarget, logWriter } = mountFirmware();
  firmware.pause();
  expect(firmware.paused()).toBe(true);
  await firmware.reloadModules();
  expect(firmware.paused()).toBe(false);
  expect(callbacks.clearInventory).toHaveBeenCalledTimes(1);
  expect(callbacks.bumpCardRefresh).toHaveBeenCalledTimes(1);
  expect(callbacks.setReloadNote).toHaveBeenLastCalledWith('Reload: nothing changed (4 ms)');
  expect(logWriter.logInput).toHaveBeenCalledWith(expect.objectContaining({ source: 'reload', level: 'info' }));

  setTarget({ kind: 'midi', port: 'USB' });
  await firmware.reloadModules();
  expect(callbacks.clearInventory).toHaveBeenCalledTimes(1);
});

test('disposal ignores a pending source read and clears the delayed card refresh after reset', async () => {
  vi.useFakeTimers();
  onTestFinished(() => {
    vi.useRealTimers();
  });
  const { firmware, callbacks, dispose, host } = mountFirmware();
  await firmware.reset();
  expect(callbacks.bumpCardRefresh).toHaveBeenCalledTimes(1);
  expect(firmware.emulatorGeneration()).toBe(2);

  let finishSource!: (value: EmuSource) => void;
  host.replies['emu.sourceInfo'] = () => new Promise<EmuSource>((resolve) => (finishSource = resolve));
  const reading = firmware.pollSource();
  dispose();
  expect(vi.getTimerCount()).toBe(0);
  finishSource(file('/tmp/late.syx'));
  await reading;
  await vi.advanceTimersByTimeAsync(2500);

  expect(firmware.source()).toBeUndefined();
  expect(callbacks.bumpCardRefresh).toHaveBeenCalledTimes(1);
});

test('the reload action only reaches the shell registered in its own window', () => {
  const first = createEmulatorActions();
  const second = createEmulatorActions();
  const closed = vi.fn();
  const reopened = vi.fn();
  const otherWindow = vi.fn();
  const unregister = first.registerReloadWithRuntime(closed);
  first.reloadWithRuntime();
  expect(closed).toHaveBeenCalledTimes(1);

  const closeAgain = first.registerReloadWithRuntime(reopened);
  second.registerReloadWithRuntime(otherWindow);
  unregister();
  first.reloadWithRuntime();
  closeAgain();
  first.reloadWithRuntime();
  second.reloadWithRuntime();

  expect(closed).toHaveBeenCalledTimes(1);
  expect(reopened).toHaveBeenCalledTimes(1);
  expect(otherWindow).toHaveBeenCalledTimes(1);
});

test('stock patched in Workbench activates the emulator package before defaults with a MIDI target selected', async () => {
  const runtimeVersion = 0x001f00;
  const bytes = Uint8Array.of(1, 2, 3);
  let ready = false;
  const session = {
    clock: wallClock,
    probe: <T>(read: (quick: Quick) => Promise<T>, quick: Quick = true) => read(quick),
    info: vi.fn(
      async () =>
        ({
          port: device,
          runtime: runtimeVersion,
          caps: 1 << CAP.MODULES,
          safe_mode: !ready,
          image_id: 'fixture',
          card_read_only: false,
        }) as DkInfoFull,
    ),
    modules: vi.fn(async () => {
      if (!ready) throw new DkError({ error: 'Package hook unavailable', kind: 'device', status: 'E_PROTO' });
      return [];
    }),
    list: vi.fn(async () => []),
    put: vi.fn(async () => {}),
    verify: vi.fn(async () => fileHashHex(bytes)),
    retry: vi.fn(async () => {}),
    restart: vi.fn(async () => void (ready = true)),
    waitForBoot: vi.fn(async () => {}),
  };
  const plan = vi.fn(async () => ['put', 'verify', 'restart', 'confirm_support'] as const);
  const systemPackage = {
    read: vi.fn(async () => ({ runtime: runtimeVersion, bytes, plan })),
    session: vi.fn(async () => session),
  };
  const { firmware, runtime, defaults, jobs } = mountFirmware({}, { kind: 'midi', port: 'USB' }, systemPackage);
  defaults.tickedFor.mockReturnValue(['test/module']);
  session.put.mockImplementation(async () => {
    expect(jobs.busy('target')).toBe(true);
    expect(jobs.busy('emulator')).toBe(true);
  });
  await firmware.bootPath('/tmp/stock.syx', 'Loaded at');
  await vi.waitFor(() => expect(defaults.install).toHaveBeenCalledOnce());
  expect(runtime.loadPath).toHaveBeenCalledBefore(systemPackage.read);
  expect(session.verify).toHaveBeenCalledBefore(session.restart);
  expect(session.restart).toHaveBeenCalledBefore(defaults.install);
  expect(session.modules).toHaveBeenCalledTimes(2);
});

test('an imported patched emulator image is left alone', async () => {
  const read = vi.fn(async () => {
    throw new Error('unexpected package read');
  });
  const session = vi.fn(async () => {
    throw new Error('unexpected session');
  });
  const { firmware, runtime } = mountFirmware({}, { kind: 'emulator' }, { read, session });
  runtime.firmwareInfo.mockResolvedValue({ runtime: { patched: false } });
  await firmware.bootPath('/tmp/imported.syx', 'Loaded at');
  expect(firmware.status()).toBe(snapshot.status);
  expect(read).not.toHaveBeenCalled();
  expect(session).not.toHaveBeenCalled();
});
