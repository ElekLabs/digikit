import { createEffect, createMemo, createSignal, onCleanup, untrack } from 'solid-js';
import { registry, runtimeDeviceIds } from '@digi/workbench-core/data/device-profiles.ts';
import type { EmuLaunch, EmuSource, Host } from '@digi/workbench-core/host/host.ts';
import { JobBusyError, type JobsApp } from '@digi/workbench-core/jobs/jobs.ts';
import { isCancelled } from '@digi/workbench-core/modules/backend.ts';
import { activateSystemPackage, type SystemPackage } from '@digi/workbench-core/runtime/activate-system-package.ts';
import { reloadError, reloadLine } from '@digi/workbench-core/runtime/reload.ts';
import type { FirmwareInfo, RuntimeSnapshot, RuntimeUpdate } from '@digi/workbench-core/runtime/runtime.ts';
import type { KeyValue } from '@digi/workbench-core/storage.ts';
import type { ConnectionApp } from '@digi/workbench-core/transfer/connection-state.ts';
import { EMULATOR_NO_RUNTIME, type TargetApp } from '@digi/workbench-core/transfer/target.ts';
import {
  booting as startBoot,
  bootingScreen,
  bootOnEmulatorPage,
  keepDeferredBoot,
  loadFailure,
  shouldBootOnStart,
  type Booting,
} from '../boot-core';
import { dspPending, hasDspChoice, readDspChoice, startupDsp, writeDspChoice } from '../dsp-core';
import { duration, failureEntry, firmwareEntry, reloadEntry, reloadFailureEntry } from '../log-core';
import type { LogWriter } from '../log-state';
import type { Page } from '../page-core';
import { createPrefs } from '../prefs';
import { createDefaultInstall, type DefaultModules } from './default-install';
import type { SessionRuntime } from './session';

const RUNTIME_KEY = 'digikit.bootRuntime';
const LOAD_ON_START_KEY = 'digikit.loadOnStart';
const LAST_FIRMWARE_KEY = 'digikit.lastFirmware';

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

type FirmwareRuntime = Pick<
  SessionRuntime,
  'loadPath' | 'firmwareInfo' | 'cancelJob' | 'reloadModules' | 'restart' | 'pause' | 'resume'
>;

type FirmwareOptions = {
  emu: Pick<Host['emu'], 'pickSyx' | 'setSource' | 'sourceInfo'>;
  storage: KeyValue;
  runtime: () => FirmwareRuntime | undefined;
  signal: AbortSignal;
  jobs: Pick<JobsApp, 'runJob' | 'busy'>;
  target: Pick<TargetApp, 'target' | 'emulatorRuntime' | 'setEmulatorRuntime' | 'setEmulatorAvailable'>;
  connection: Pick<ConnectionApp, 'noteConnection'>;
  defaults: DefaultModules;
  systemPackage?: { read(): Promise<SystemPackage>; session(): Promise<Parameters<typeof activateSystemPackage>[0]> };
  logWriter: Pick<LogWriter, 'logEvent' | 'logInput'>;
  page: () => Page;
  showSnapshot: (snapshot: RuntimeSnapshot) => void;
  releaseHeldKeys: () => void;
  setError: (error: string) => void;
  setReloadNote: (note: string) => void;
  watchSnapshotError: (error: string | null) => void;
  resetHealth: () => void;
  controllerBusy: () => boolean;
  setBrowseDevice: (device: string) => void;
  clearInventory: () => void;
  bumpCardRefresh: () => void;
};

/** Firmware commands and state for one Workbench owner and its native session. */
export function createEmulatorFirmware(options: FirmwareOptions) {
  const {
    emu,
    storage,
    runtime,
    signal,
    defaults,
    systemPackage: packageSource,
    page,
    showSnapshot,
    releaseHeldKeys,
    setError,
    setReloadNote,
    watchSnapshotError,
    resetHealth,
    controllerBusy,
    setBrowseDevice,
    clearInventory,
    bumpCardRefresh,
  } = options;
  const { runJob, busy } = options.jobs;
  const { target, emulatorRuntime, setEmulatorRuntime, setEmulatorAvailable } = options.target;
  const { noteConnection } = options.connection;
  const { logInput, logEvent } = options.logWriter;
  const { readPref, writePref } = createPrefs(storage);

  const [status, setStatus] = createSignal<RuntimeSnapshot['status']>();
  const [paused, setPaused] = createSignal(false);
  const [source, setSource] = createSignal<EmuSource>();
  const [booting, setBooting] = createSignal<Booting>();
  const [lastDevice, setLastDevice] = createSignal<string>();
  const [failed, setFailed] = createSignal<Booting>();
  const [watching, setWatching] = createSignal(true);
  const [bootRuntime, setBootRuntime] = createSignal(readPref(RUNTIME_KEY) !== 'off');
  const [bootDsp, setBootDsp] = createSignal(readDspChoice(storage));
  const [loadOnStart, setLoadOnStart] = createSignal(readPref(LOAD_ON_START_KEY) === 'on');
  const [info, setInfo] = createSignal<FirmwareInfo>();
  const [emulatorGeneration, setEmulatorGeneration] = createSignal(1);
  let lastModified: number | undefined;

  const cancelControllerJob = () => void runtime()?.cancelJob?.();
  const installDefaultModules = createDefaultInstall({
    defaults,
    runJob,
    signal,
    cancelControllerJob,
    controllerBusy,
    setError,
    setReloadNote,
    logInput,
    bumpCardRefresh,
  });

  const chooseRuntime = (on: boolean) => {
    if (signal.aborted) return;
    setBootRuntime(on);
    writePref(RUNTIME_KEY, on ? 'on' : 'off');
  };
  const chooseDsp = (on: boolean) => {
    if (signal.aborted) return;
    setBootDsp(on);
    writeDspChoice(storage, on);
  };
  const chooseLoadOnStart = (on: boolean) => {
    if (signal.aborted) return;
    setLoadOnStart(on);
    writePref(LOAD_ON_START_KEY, on ? 'on' : 'off');
  };
  const toggleWatching = () => {
    if (!signal.aborted) setWatching((on) => !on);
  };

  const cardTimers = new Set<ReturnType<typeof setTimeout>>();
  const refreshCardAfterQuietPeriod = () => {
    const timer = setTimeout(() => {
      cardTimers.delete(timer);
      if (!signal.aborted) bumpCardRefresh();
    }, 2500);
    cardTimers.add(timer);
  };
  onCleanup(() => cardTimers.forEach((timer) => clearTimeout(timer)));

  // While an image boots, only its own device counts: an unknown image has none (no stale device, no faceplate).
  const knownDevice = createMemo(
    () => status()?.device ?? (booting() ? booting()!.device : (failed()?.device ?? lastDevice())),
  );

  /** A device without `[digikit]` has no runtime, no modules and no remote: it starts stock, and those pages say so. */
  const hasRuntime = (id: string | undefined) => id === undefined || runtimeDeviceIds.includes(id);
  const runtimeHere = () => hasRuntime(knownDevice());
  /** The device has a DSP or a second CPU with a stand-in: the "DSP" tick applies (unknown device: yes). */
  const dspHere = () => {
    const id = knownDevice();
    return id === undefined || !registry.ids.includes(id) || hasDspChoice(registry.get(id));
  };
  /** The tick differs from the running image: it applies at the next Reload or Open. */
  const dspIsPending = () => dspHere() && dspPending(bootDsp(), info()?.dsp);

  const emulatorLoaded = createMemo(() => !!status());
  createEffect(() => {
    setEmulatorAvailable(emulatorLoaded() && runtimeHere());
  });

  // The "Auto-load firmware" boot that the start deferred because the target was a device.
  const [deferredBoot, setDeferredBoot] = createSignal<string>();
  createEffect(() => {
    const path = deferredBoot();
    if (signal.aborted) return;
    if (path && !keepDeferredBoot(!!status(), !!booting())) {
      setDeferredBoot(undefined);
      return;
    }
    if (path && bootOnEmulatorPage(true, page(), target().kind, !!status(), !!booting())) {
      setDeferredBoot(undefined);
      void bootPath(path, 'Loaded at');
    }
  });

  const update = (event: RuntimeUpdate) => {
    if (signal.aborted) return;
    if (event.type === 'error') {
      setError(event.error);
      return;
    }
    showSnapshot(event.snapshot);
    setStatus(event.snapshot.status);
    const running = event.snapshot.status.device;
    // the module browser follows the emulator's device when it is one modules are built for
    if (target().kind === 'emulator' && lastDevice() !== running && registry.runtimeIds.includes(running))
      setBrowseDevice(running);
    setLastDevice(event.snapshot.status.device);
    setError(event.snapshot.status.error ?? '');
    watchSnapshotError(event.snapshot.status.error);
  };

  // The emulator has the runtime when the image has a runtime record (patched now, from the cache, or a patched file loaded as it is);
  // unknown while booting.
  createEffect(() => {
    const i = info();
    setEmulatorRuntime(i === undefined ? 'unknown' : i.runtime ? 'yes' : 'no');
  });
  /** The tick differs from what the running image has: it applies at the next Reload or Open. Tick on: pending when the image has no
   * runtime. Tick off: pending when this boot patched the runtime in (a file loaded as it is, with its own runtime, keeps it either way). */
  const runtimePending = () => {
    const i = info();
    if (i === undefined || !runtimeHere()) return false;
    return bootRuntime() ? !i.runtime : !!i.runtime?.patched;
  };
  /** What the running image got from the runtime patch, for the header. */
  const refreshInfo = async () => {
    if (signal.aborted) return;
    try {
      const next = await runtime()?.firmwareInfo?.();
      if (!signal.aborted) setInfo(next);
    } catch {
      if (!signal.aborted) setInfo(undefined);
    }
  };
  /** Start a new firmware session through the controller's `firmware.load`. */
  const boot = async (path: string, file: EmuSource | null, choices: { runtime: boolean; card?: string }) => {
    const r = runtime();
    if (!r || signal.aborted) return;
    const name = file?.name ?? path;
    const starting = startBoot(name, file?.device ?? undefined);
    releaseHeldKeys();
    setEmulatorGeneration((n) => n + 1);
    setError('');
    setStatus(undefined);
    setInfo(undefined);
    // a boot or reset of the emulator: the indicator asks again
    if (target().kind === 'emulator') noteConnection({ type: 'target' });
    setReloadNote('');
    setBooting(starting);
    logInput({ source: 'firmware', level: 'info', text: bootingScreen(starting) });
    setFailed(undefined);
    resetHealth();
    const began = performance.now();
    try {
      if (!r.loadPath) throw new Error('no reload');
      // The controller applies these choices using the new image's device data.
      const snapshot = await r.loadPath(path, { ...choices, dsp: bootDsp() });
      if (signal.aborted) return false;
      update({ type: 'snapshot', snapshot });
      setPaused(false);
      await refreshInfo();
      if (signal.aborted) return false;
      if (target().kind === 'emulator' && emulatorRuntime() === 'no')
        noteConnection({ type: 'noruntime', reason: EMULATOR_NO_RUNTIME });
      const s = status();
      if (s)
        logInput(
          firmwareEntry({
            name,
            device: s.device,
            version: s.version,
            runtime: choices.runtime,
            patch: info()?.runtime,
            ms: performance.now() - began,
          }),
        );
      // another firmware may be another device: its card folder and its memory (the runtime needs a moment to load its modules)
      bumpCardRefresh();
      refreshCardAfterQuietPeriod();
      return true;
    } catch (e) {
      if (signal.aborted) return false;
      if (isCancelled(e) || isCancelled((e as { cause?: unknown })?.cause)) {
        setReloadNote(`Load cancelled: ${name}`);
        logInput({ source: 'firmware', level: 'info', text: `loading ${name} cancelled` });
        if (status()) await refreshInfo();
        return false;
      }
      const detail = message(e);
      setError(detail);
      setFailed(starting);
      setReloadNote(loadFailure(name, detail));
      logInput(failureEntry('firmware', `loading ${name}`, detail));
      if (status()) await refreshInfo(); // A refused load can restore the previous live session.
      return false;
    } finally {
      if (!signal.aborted) setBooting(undefined);
    }
  };
  /** Make `path` the console's firmware file and boot it; the controller reads it from its path. */
  const bootPath = async (path: string, why: string, options: { runtime?: boolean; card?: string } = {}) => {
    if (signal.aborted) return;
    const withRuntime = options.runtime ?? bootRuntime();
    const base = path.split(/[\\/]/).pop() || path;
    const packageAbort = new AbortController();
    const packageSignal = AbortSignal.any([signal, packageAbort.signal]);
    const cancel = () => {
      packageAbort.abort();
      cancelControllerJob();
    };
    try {
      const loaded = await runJob(`Loading ${base}`, { group: 'emulator', cancel }, async () => {
        if (signal.aborted) return false;
        const file = await emu.setSource(path);
        if (signal.aborted) return false;
        setSource(file ?? undefined);
        lastModified = file?.modified_ms;
        const rBeforeLoad = runtime();
        const ok = await untrack(() => boot(path, file, { runtime: withRuntime, card: options.card }));
        if (ok && !signal.aborted) {
          writePref(LAST_FIRMWARE_KEY, path);
          setReloadNote(`${why} ${new Date().toLocaleTimeString()}`);
        }
        const device = status()?.device;
        if (ok && device && info()?.runtime?.patched && packageSource && !signal.aborted) {
          const generation = emulatorGeneration();
          const check = () => {
            if (generation !== emulatorGeneration() || rBeforeLoad !== runtime())
              throw new Error('The emulator firmware changed.');
          };
          try {
            const changed = await runJob('Installing system package', { group: 'target', cancel }, async (job) => {
              const pkg = await packageSource.read();
              packageSignal.throwIfAborted();
              check();
              return activateSystemPackage(await packageSource.session(), pkg, {
                identity: { device },
                signal: packageSignal,
                check,
                progress: job.progress,
              });
            });
            logInput({
              source: 'firmware',
              level: 'info',
              text: changed ? 'System package verified and active.' : 'System package already active.',
            });
          } catch (e) {
            if (!signal.aborted) {
              const problem = packageSignal.aborted
                ? 'Setup was stopped. The package may already be on the card.'
                : e instanceof JobBusyError
                  ? 'The target is busy.'
                  : message(e);
              const detail = `${problem} If the target is still core-only, choose Transfer → Modules → Install system package. Otherwise reload the firmware again.`;
              setReloadNote(`System package setup failed. ${detail}`);
              logInput(failureEntry('firmware', 'system package setup', detail));
            }
            return false;
          } finally {
            if (!signal.aborted) bumpCardRefresh();
          }
        }
        return ok;
      });
      // the install is its own job in the same group: it starts after the load job ended
      const s = status();
      if (loaded && s && withRuntime && !signal.aborted) void installDefaultModules(s.device);
    } catch (e) {
      if (signal.aborted || e instanceof JobBusyError) return;
      setError(message(e));
    }
  };
  /** Boot the console's firmware file again. */
  const reload = async (why: string) => {
    const path = source()?.path;
    if (path) await bootPath(path, why);
  };
  /** The native picker; the picked file becomes the console's file and boots. */
  const openFirmware = async () => {
    if (signal.aborted) return;
    try {
      const path = await emu.pickSyx(source()?.path ?? readPref(LAST_FIRMWARE_KEY));
      if (path) await bootPath(path, 'Opened at');
    } catch (error) {
      if (!signal.aborted) throw error;
    }
  };
  /** `dev.reload {build: true}`: the controller builds the stale ColdFire objects, applies the modules again from the objects on disk
   * and swaps module code into a restored main screen, or boots the new image; the result goes to the status area and the Log. A build
   * error is one short line in the status area; the Log has it with the compiler output as its detail. */
  const reloadModules = async () => {
    const r = runtime();
    if (signal.aborted || !r?.reloadModules) return;
    try {
      await runJob('Reloading modules', { group: 'emulator' }, async () => {
        setError('');
        setReloadNote('Reloading modules…');
        if (target().kind === 'emulator') clearInventory();
        try {
          const report = await r.reloadModules!();
          if (signal.aborted) return;
          setReloadNote(reloadLine(report));
          logInput(reloadEntry(report));
          resetHealth();
          setPaused(false);
          await refreshInfo();
        } catch (e) {
          if (signal.aborted) return;
          if (isCancelled(e)) {
            setReloadNote('Module reload cancelled');
            logInput({ source: 'firmware', level: 'info', text: 'module reload cancelled' });
            return;
          }
          const failure = reloadError(e);
          setReloadNote(failure.status);
          logInput(reloadFailureEntry(failure));
        } finally {
          if (!signal.aborted) bumpCardRefresh();
        }
      });
    } catch (e) {
      if (!signal.aborted && !(e instanceof JobBusyError)) setError(message(e));
    }
  };
  /** Apply the runtime choice and reload from the action offered on other pages. */
  const reloadWithRuntime = () => {
    chooseRuntime(true);
    void reload('Reloaded with the runtime at');
  };
  const pollSource = async () => {
    if (signal.aborted) return;
    try {
      const file = await emu.sourceInfo();
      if (signal.aborted) return;
      setSource(file ?? undefined);
      if (!file) return;
      const changed = lastModified !== undefined && file.modified_ms !== lastModified;
      lastModified = file.modified_ms;
      if (changed && watching()) await reload('Reloaded after a change at');
    } catch {
      /* the file may be mid-write; the next poll sees it */
    }
  };

  const applyLaunch = async (launch: EmuLaunch) => {
    if (launch.loaded) await pollSource();
    // The launcher's `--no-dsp` / `--dsp-on` wins over the saved choice for this session, also for the first load below; only a
    // click on the tick saves a choice. The controller reports the flag before any firmware is loaded.
    await refreshInfo();
    if (signal.aborted) return;
    const dsp = startupDsp(bootDsp(), info()?.dsp);
    if (dsp !== bootDsp()) setBootDsp(dsp);
    if (launch.loaded) {
      if (dspIsPending()) await reload(`Started with DSP ${dsp ? 'on' : 'off'} at`);
      return;
    }
    // A command-line file, else the last opened file when "Auto-load firmware" is ticked and the target is the emulator, boots now.
    // With a device as target the last file waits for the first visit of the Emulator page.
    const last = readPref(LAST_FIRMWARE_KEY);
    if (shouldBootOnStart(launch.syx, loadOnStart(), target().kind)) {
      const path = launch.syx ?? last;
      if (path)
        await bootPath(path, 'Loaded at', {
          runtime: launch.runtime ?? undefined,
          card: launch.card ?? undefined,
        });
    } else if (loadOnStart() && last) {
      setDeferredBoot(last);
    }
  };

  const setEmulatorPaused = (pause: boolean) => {
    const r = runtime();
    if (signal.aborted || !r || busy('emulator')) return;
    if (pause) r.pause();
    else r.resume();
    setPaused(pause);
    logEvent('emulator', 'info', paused() ? 'paused' : 'resumed');
  };
  const reset = async () => {
    if (signal.aborted || busy('emulator')) return;
    // a boot or reset of the emulator: the indicator asks again
    if (target().kind === 'emulator') noteConnection({ type: 'target' });
    resetHealth();
    const began = performance.now();
    try {
      const s = await runtime()?.restart();
      if (signal.aborted) return;
      if (s) {
        setEmulatorGeneration((n) => n + 1);
        update({ type: 'snapshot', snapshot: s });
      }
      setPaused(false);
      await refreshInfo();
      if (signal.aborted) return;
      if (target().kind === 'emulator' && emulatorRuntime() === 'no')
        noteConnection({ type: 'noruntime', reason: EMULATOR_NO_RUNTIME });
      logEvent('reset', 'info', `emulator reset · ${duration(performance.now() - began)}`);
      bumpCardRefresh();
      refreshCardAfterQuietPeriod();
    } catch (e) {
      if (signal.aborted) return;
      setError(message(e));
      logInput(failureEntry('reset', 'reset', message(e)));
    }
  };

  return {
    status,
    emulatorLoaded,
    paused,
    source,
    booting,
    knownDevice,
    watching,
    bootRuntime,
    bootDsp,
    loadOnStart,
    info,
    emulatorGeneration,
    runtimeHere,
    runtimePending,
    dspHere,
    dspIsPending,
    chooseRuntime,
    chooseDsp,
    chooseLoadOnStart,
    toggleWatching,
    update,
    applyLaunch,
    pollSource,
    bootPath,
    openFirmware,
    reload,
    reloadWithRuntime,
    reloadModules,
    pause: () => setEmulatorPaused(true),
    resume: () => setEmulatorPaused(false),
    reset,
  };
}
