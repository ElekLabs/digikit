// The desktop developer console: the file the app runs, the live panel, and the audio and CPU meters.
// `mise run workbench -- FILE` starts it on FILE; with "watching" on, a change to FILE on disk reloads it, so a
// module can be patched from the command line and heard here a second later.
import {
  type ComponentProps,
  For,
  Show,
  batch,
  lazy,
  Suspense,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  untrack,
} from 'solid-js';
import LivePanel, { type LivePanelApi } from '@digi/workbench-core/ui/components/LivePanel.tsx';
import { defaultFaceplate, panels } from '@digi/workbench-core/data/faceplates.ts';
import { deviceNameOf, registry, runtimeDeviceIds, type DeviceId } from '@digi/workbench-core/data/device-profiles.ts';
import {
  type EmulatorRuntime,
  type FirmwareInfo,
  type NativeAudioStatus,
  type RuntimeSnapshot,
  type RuntimeUpdate,
} from '@digi/workbench-core/runtime/runtime.ts';
import {
  booting as startBoot,
  bootingScreen,
  bootingStatus,
  bootOnEmulatorPage,
  keepDeferredBoot,
  loadFailure,
  shouldBootOnStart,
  type Booting,
} from './boot-core';
import { bumpCardRefresh, cardRefresh } from './card-state';
import CardPanel from './CardPanel';
import { connectionView } from '@digi/workbench-core/transfer/connection-core.ts';
import { targetLabel } from '@digi/workbench-core/transfer/target.ts';
import { createTargetHealth } from './target-health';
import type { SpanRecording } from './span-recording';
import type { PerformanceView } from './PerformancePanel';
import { createUtilizationHistory } from './utilization-history';
import { percent } from './device-health';
import { pageFor, type Page } from './page-core';
import {
  CHART,
  DOWN,
  FIRMWARE,
  FOLDER,
  Icon,
  MODULES,
  PAUSE,
  PLAY,
  RELOAD,
  REMOTE,
  RESTART,
  SETTINGS,
  SPEAKER,
  UP,
  WARNING,
} from './icons';
import { DOCK_KEY_STEP, DOCK_MIN_PX, clampDock, dockAt, dockMax, parseDock, startDock } from './dock-core';
import {
  duration,
  failureEntry,
  firmwareEntry,
  healthChanges,
  healthStart,
  reloadEntry,
  reloadFailureEntry,
  unseenErrors,
} from './log-core';
import { createNativeRuntime } from './native-runtime';
import { themes, type createTheme } from './theme';
import { createPrefs } from './prefs';
import { pgoWarning, type BuildInfo } from './pgo-warning';
import { createLogFollow, followSession, type FollowEnv } from './log-follow';
import { createDkProfLoader } from './dkprof-env';
import { createDeviceNames, createEmulatorNames, selectedSymbolImage } from './log-names-backend';
import { windowShown } from './log-follow-core';
import { followOn, logEvent, logInput, logOpen, logState, setLogOpen } from './log-state';
import LogPanel from './LogPanel';
import FirmwareSymbols from './FirmwareSymbols';
import type { EmuSource as Source, Host } from '@digi/workbench-core/host/host.ts';
import SidePanels from './SidePanels';
import TitleBar from './TitleBar';
import { setInspectorOpen, setInspectorTab } from './panel-open';
import RemotePage from './RemotePage';
import RemoteAudio from './RemoteAudio';
import type { WorkerFactory } from '@digi/workbench-core/workers.ts';
import { useFirmwareApp, useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { EMULATOR_NO_RUNTIME } from '@digi/workbench-core/transfer/target.ts';
import { setReloadWithRuntime } from './emulator-actions';
import { dspLine, dspPending, hasDspChoice, readDspChoice, startupDsp, writeDspChoice } from './dsp-core';
import { running as flashRunning } from '@digi/workbench-core/firmware/firmware-core.ts';
import FirmwarePage from '@digi/workbench-core/ui/firmware/FirmwarePage.tsx';
import DefaultModules from '@digi/workbench-core/ui/firmware/DefaultModules.tsx';
import { DefaultModuleError } from '@digi/workbench-core/firmware/default-modules-core.ts';
import TransferPage from '@digi/workbench-core/ui/transfer/TransferPage.tsx';
import { targetOps } from '@digi/workbench-core/target-ops/session-store.ts';
import FilesPage from '@digi/workbench-core/ui/files/FilesPage.tsx';
import { cardLine, folderFiles, startCard, type CardStatus } from '@digi/workbench-core/runtime/cards.ts';
import { reloadError, reloadLine } from '@digi/workbench-core/runtime/reload.ts';
import type { ControllerJob } from '@digi/workbench-core/jobs/jobs-core.ts';
import { isCancelled } from '@digi/workbench-core/modules/backend.ts';
import { JobBusyError, type JobHandle } from '@digi/workbench-core/jobs/jobs.ts';

type Status = RuntimeSnapshot['status'];

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

// "Boot with digikit runtime": on unless the user turned it off; kept per browser profile of the app.
const RUNTIME_KEY = 'digikit.bootRuntime';

// "Load on start": boot the last opened firmware file when the app opens (off unless ticked), and that file's path.
const LOAD_ON_START_KEY = 'digikit.loadOnStart';
const LAST_FIRMWARE_KEY = 'digikit.lastFirmware';

// The height of the Log dock in px; kept per browser profile of the app (none saved: a share of the window).
const DOCK_KEY = 'digikit.logHeight';

// The sidebar is open unless the user collapsed it; kept per browser profile of the app.
const SIDEBAR_KEY = 'digikit.sidebar';

function Meter(props: { value: number; worst?: number; tone?: 'teal' | 'amber' }) {
  const pct = (v: number) => `${Math.max(0, Math.min(100, v))}%`;
  return (
    <span class="meter" aria-hidden="true">
      <b class={props.tone ?? 'teal'} style={{ width: pct(props.value) }} />
      <Show when={props.worst !== undefined}>
        <u style={{ left: pct(props.worst!) }} />
      </Show>
    </span>
  );
}

const PerformancePanel = lazy(() => import('./PerformancePanel'));
const AnalyzerPanel = lazy(() => import('./AnalyzerPanel'));

export interface ConsoleOptions {
  initialPage?: Page;
  files?: Pick<ComponentProps<typeof FilesPage>, 'store' | 'library'>;
  logSources?: Pick<FollowEnv, 'emulator' | 'device'>;
  initialDockHeight?: number;
}

export default function Console(
  props: ConsoleOptions & {
    theme: ReturnType<typeof createTheme>;
    host: Host;
    analyzerWorkers: WorkerFactory<'analyzer'>;
  },
) {
  const app = useTransferApp();
  const fw = useFirmwareApp();
  const { readPref, writePref } = createPrefs(app.storage);
  const readRuntimePref = () => readPref(RUNTIME_KEY) !== 'off';
  const writeRuntimePref = (on: boolean) => writePref(RUNTIME_KEY, on ? 'on' : 'off');
  const readDockPref = () => parseDock(readPref(DOCK_KEY));
  const writeDockPref = (px: number | undefined) => writePref(DOCK_KEY, px === undefined ? null : String(px));
  const readSidebarPref = () => readPref(SIDEBAR_KEY) !== 'closed';
  const writeSidebarPref = (open: boolean) => writePref(SIDEBAR_KEY, open ? 'open' : 'closed');
  // "DSP": on unless the user turned it off; kept like the runtime tick (dsp-core.ts).
  const readDspPref = () => readDspChoice(app.storage);
  const writeDspPref = (on: boolean) => writeDspChoice(app.storage, on);
  const { emulatorRuntime, emulatorSession, lastMidiPort, lastMidiSession, setEmulatorRuntime, target } = app.target;
  const { noteConnection } = app.connection;
  const { busy, currentJob, jobs, runJob, syncControllerJob } = app.jobs;
  const { setBrowseDevice } = app.state;
  const { tickedFor } = fw.defaults;
  const flashRun = fw.state.run;
  const [runtime, setRuntime] = createSignal<EmulatorRuntime>();
  const [controllerJob, setControllerJob] = createSignal<ControllerJob | null>(null);
  const [outsideApp, setOutsideApp] = createSignal(false);
  const [status, setStatus] = createSignal<Status>();
  const [error, setError] = createSignal('');
  const [paused, setPaused] = createSignal(false);
  const [source, setSource] = createSignal<Source>();
  /** A firmware is starting: the old device information is gone until its first status. */
  const [booting, setBooting] = createSignal<Booting>();
  /** The last load failed: its file, so the faceplate is the image's device, or none when the image does not tell. */
  /** The device that was showing: an image of no known device keeps that faceplate. */
  const [lastDevice, setLastDevice] = createSignal<string>();
  const [failed, setFailed] = createSignal<Booting>();
  const [watching, setWatching] = createSignal(true);
  const [reloadNote, setReloadNote] = createSignal('');
  const [now, setNow] = createSignal(Date.now());
  const [load_, setLoad] = createSignal<NativeAudioStatus['load']>();
  const [audio, setAudio] = createSignal<NativeAudioStatus>();
  const [buildInfo, setBuildInfo] = createSignal<BuildInfo>();
  const [rate, setRate] = createSignal<number>();
  // the Log drawer: the last entry seen (the badge counts the errors after it); the filter and the list are in LogPanel.tsx
  const [seenId, setSeenId] = createSignal(0);
  // the Log dock: its height (undefined: the default share of the window), the window height, and a drag in progress
  const [dockSaved, setDockSaved] = createSignal(props.initialDockHeight ?? readDockPref());
  const [windowPx, setWindowPx] = createSignal(window.innerHeight);
  const [dockDrag, setDockDrag] = createSignal(false);
  const [page, setPage] = createSignal<Page>(props.initialPage ?? 'run');
  const [sidebarOpen, setSidebarOpen] = createSignal(readSidebarPref());
  const [card, setCard] = createSignal<CardStatus>();
  const [bootRuntime, setBootRuntime] = createSignal(readRuntimePref());
  const [bootDsp, setBootDsp] = createSignal(readDspPref());
  const [loadOnStart, setLoadOnStart] = createSignal(readPref(LOAD_ON_START_KEY) === 'on');
  const [info, setInfo] = createSignal<FirmwareInfo>();
  let live: LivePanelApi | undefined;
  let folderInput: HTMLInputElement | undefined;
  let lastAudio: { produced: number; wall: number } | undefined;
  let lastModified: number | undefined;
  let health = healthStart();
  let dockEl: HTMLElement | undefined;
  const timers: number[] = [];

  // a memo: `status` is set once per display frame, and the panels must hear of a change of this value only
  const sidePanelsShown = createMemo(() => page() === 'run');
  const selected = createMemo(() =>
    targetOps(app.target, {
      pause: async () => setEmulatorPaused(true),
      resume: async () => setEmulatorPaused(false),
      reset: () => restart(),
    }),
  );
  const isEmulator = () => selected().target.kind === 'emulator';
  const [emulatorGeneration, setEmulatorGeneration] = createSignal(1);
  const emulatorLoaded = createMemo(() => !!status());
  const connectionState = createMemo(() => app.connection.connection().state);
  const symbolSource = () =>
    app.target.switching() || busy('target') || (isEmulator() && !emulatorLoaded()) ? undefined : selected();
  createEffect(() => {
    if (!isEmulator()) untrack(() => live?.releaseAll());
  });
  const connection = () => connectionView(app.connection.connection(), targetLabel(target()));
  const remoteHealth = createTargetHealth(
    selected,
    () => !app.target.switching() && !busy('target') && app.connection.connection().state === 'connected',
  );
  const utilization = createUtilizationHistory(selected, remoteHealth);
  // While an image boots, only its own device counts: an unknown image has none (no stale device, no faceplate).
  const knownDevice = createMemo(
    () => status()?.device ?? (booting() ? booting()!.device : (failed()?.device ?? lastDevice())),
  );
  // a memo, so its readers re-run only when the id changes
  const device = createMemo(() => {
    const d = knownDevice();
    return d !== undefined && Object.hasOwn(panels, d) ? d : defaultFaceplate;
  });
  const panel = () => panels[device()];
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
  const noRuntimeTitle = () => `${deviceNameOf(device())} has no digikit runtime: not available on this device`;
  createEffect(() => {
    setPage(pageFor(page(), target(), !!status() && runtimeHere()));
  });
  // The "Load on start" boot that the start deferred because the target was a device.
  const [deferredBoot, setDeferredBoot] = createSignal<string>();
  createEffect(() => {
    const path = deferredBoot();
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
    if (event.type === 'error') {
      setError(event.error);
      return;
    }
    live?.show(event.snapshot);
    setStatus(event.snapshot.status);
    const running = event.snapshot.status.device;
    // the module browser follows the emulator's device when it is one modules are built for
    if (target().kind === 'emulator' && lastDevice() !== running && registry.runtimeIds.includes(running))
      setBrowseDevice(running);
    setLastDevice(event.snapshot.status.device);
    setError(event.snapshot.status.error ?? '');
    watchHealth({ error: event.snapshot.status.error });
  };
  /** Log a guest fault, stall or DSP halt when it starts (the app polls the status; it gets no events from the controller). */
  const watchHealth = (reading: Parameters<typeof healthChanges>[1]) => {
    const seen = healthChanges(health, reading);
    health = seen.health;
    seen.entries.forEach(logInput);
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
    try {
      setInfo(await runtime()?.firmwareInfo?.());
    } catch {
      setInfo(undefined);
    }
  };
  /**
   * The ticked default modules of the device (devices/*.toml `[digikit] default_modules`) go on the emulator's card and are enabled, so the
   * emulator has what a flashed device has. The card keeps them: a later load finds them there and only checks the files.
   */
  const installDefaultModules = async (deviceId: string): Promise<void> => {
    try {
      const stop = new AbortController();
      await runJob(
        'Installing default modules',
        {
          group: 'emulator',
          cancel: () => {
            stop.abort();
            void runtime()?.cancelJob?.();
          },
        },
        (job) => performDefaultInstall(deviceId, job, stop.signal),
      );
    } catch (e) {
      if (!(e instanceof JobBusyError)) setError(message(e));
    }
  };
  const performDefaultInstall = async (deviceId: string, job: JobHandle, signal: AbortSignal) => {
    const ids = tickedFor(deviceId);
    if (ids.length === 0) return;
    try {
      const lines = await fw.env.defaultModules!.install(
        'emulator',
        deviceId,
        ids,
        (p) => job.progress(undefined, p.text),
        { signal, stillBooting: () => controllerJob() !== null },
      );
      setReloadNote(`Default modules ready: ${ids.join(', ')}`);
      logInput({
        source: 'firmware',
        level: 'info',
        text: `default modules in the emulator: ${ids.join(', ')}`,
        detail: lines.join('\n'),
      });
    } catch (e) {
      if (isCancelled(e) || isCancelled((e as { cause?: unknown })?.cause)) {
        logInput({ source: 'firmware', level: 'info', text: 'default modules cancelled' });
        bumpCardRefresh();
        return;
      }
      const what = e instanceof DefaultModuleError ? `default module ${e.id}` : 'default modules';
      setReloadNote(`${what} failed`);
      logInput(failureEntry('firmware', what, e instanceof DefaultModuleError ? e.message : message(e)));
    }
    bumpCardRefresh();
  };
  /** Start a new firmware session with `start` (every load goes through the controller's `firmware.load`). */
  const boot = async (
    start: (r: EmulatorRuntime) => Promise<RuntimeSnapshot>,
    name: string,
    withRuntime = bootRuntime(),
    imageDevice?: DeviceId,
  ) => {
    const r = runtime();
    if (!r) return;
    live?.releaseAll();
    setEmulatorGeneration((n) => n + 1);
    setError('');
    setStatus(undefined);
    setInfo(undefined);
    // a boot or reset of the emulator: the indicator asks again
    if (target().kind === 'emulator') noteConnection({ type: 'target' });
    setReloadNote('');
    setBooting(startBoot(name, imageDevice));
    logInput({ source: 'firmware', level: 'info', text: bootingScreen(startBoot(name, imageDevice)) });
    setFailed(undefined);
    health = healthStart();
    const began = performance.now();
    try {
      update({ type: 'snapshot', snapshot: await start(r) });
      setPaused(false);
      await refreshInfo();
      if (target().kind === 'emulator' && emulatorRuntime() === 'no')
        noteConnection({ type: 'noruntime', reason: EMULATOR_NO_RUNTIME });
      const s = status();
      if (s)
        logInput(
          firmwareEntry({
            name,
            device: s.device,
            version: s.version,
            runtime: withRuntime,
            patch: info()?.runtime,
            ms: performance.now() - began,
          }),
        );
      // another firmware may be another device: its card folder and its memory (the runtime needs a moment to load its modules)
      bumpCardRefresh();
      window.setTimeout(bumpCardRefresh, 2500);
      return true;
    } catch (e) {
      if (isCancelled(e) || isCancelled((e as { cause?: unknown })?.cause)) {
        setReloadNote(`Load cancelled: ${name}`);
        logInput({ source: 'firmware', level: 'info', text: `loading ${name} cancelled` });
        if (status()) await refreshInfo();
        return false;
      }
      failLoad(name, imageDevice, e);
      if (status()) await refreshInfo(); // A refused load can restore the previous live session.
      return false;
    } finally {
      setBooting(undefined);
    }
  };
  const failLoad = (name: string, imageDevice: DeviceId | undefined, e: unknown) => {
    setError(message(e));
    setFailed(startBoot(name, imageDevice));
    setReloadNote(loadFailure(name, message(e)));
    logInput(failureEntry('firmware', `loading ${name}`, message(e)));
  };
  /** Make `path` the console's firmware file and boot it; the controller reads it from its path. */
  const bootPath = async (path: string, why: string, options: { runtime?: boolean; card?: string } = {}) => {
    const withRuntime = options.runtime ?? bootRuntime();
    const base = path.split(/[\\/]/).pop() || path;
    try {
      const loaded = await runJob(
        `Loading ${base}`,
        { group: 'emulator', cancel: () => void runtime()?.cancelJob?.() },
        async () => {
          const file = await props.host.emu.setSource(path);
          setSource(file ?? undefined);
          lastModified = file?.modified_ms;
          const ok = await boot(
            (r) =>
              r.loadPath
                ? // The file at `path` may belong to another device than the running one: send the choices as they are; the
                  // controller applies them by the loaded firmware's device data (runtime only on a runtime target, dsp only
                  // where the device offers the choice).
                  r.loadPath(path, { runtime: withRuntime, card: options.card, dsp: bootDsp() })
                : Promise.reject(new Error('no reload')),
            file?.name ?? path,
            withRuntime,
            file?.device ?? undefined,
          );
          if (ok) {
            writePref(LAST_FIRMWARE_KEY, path);
            setReloadNote(`${why} ${new Date().toLocaleTimeString()}`);
          }
          return ok;
        },
      );
      // the install is its own job in the same group: it starts after the load job ended
      const s = status();
      if (loaded && s && withRuntime) void installDefaultModules(s.device);
    } catch (e) {
      if (e instanceof JobBusyError) return;
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
    const path = await props.host.emu.pickSyx(source()?.path ?? readPref(LAST_FIRMWARE_KEY));
    if (path) await bootPath(path, 'Opened at');
  };
  /** `dev.reload {build: true}`: the controller builds the stale ColdFire objects, applies the modules again from the objects on disk
   * and swaps module code into a restored main screen, or boots the new image; the result goes to the status area and the Log. A build
   * error is one short line in the status area; the Log has it with the compiler output as its detail. */
  const reloadModules = async () => {
    const r = runtime();
    if (!r?.reloadModules) return;
    try {
      await runJob('Reloading modules', { group: 'emulator' }, async () => {
        setError('');
        setReloadNote('Reloading modules…');
        try {
          const report = await r.reloadModules!();
          setReloadNote(reloadLine(report));
          logInput(reloadEntry(report));
          health = healthStart();
          setPaused(false);
          await refreshInfo();
        } catch (e) {
          if (isCancelled(e)) {
            setReloadNote('Module reload cancelled');
            logInput({ source: 'firmware', level: 'info', text: 'module reload cancelled' });
            return;
          }
          const failure = reloadError(e);
          setReloadNote(failure.status);
          logInput(reloadFailureEntry(failure));
        }
      });
    } catch (e) {
      if (!(e instanceof JobBusyError)) setError(message(e));
    }
  };
  // the "Boot with runtime" tick of other pages' "Reload with runtime" button
  setReloadWithRuntime(() => {
    setBootRuntime(true);
    writeRuntimePref(true);
    void reload('Reloaded with the runtime at');
  });
  const pollSource = async () => {
    try {
      const file = await props.host.emu.sourceInfo();
      setSource(file ?? undefined);
      if (!file) return;
      const changed = lastModified !== undefined && file.modified_ms !== lastModified;
      lastModified = file.modified_ms;
      if (changed && watching()) await reload('Reloaded after a change at');
    } catch {
      /* the file may be mid-write; the next poll sees it */
    }
  };
  const pollAudio = async () => {
    try {
      const a = await runtime()?.nativeAudio?.();
      setAudio(a);
      watchHealth({ halted: a?.halted ?? null });
      const produced = a?.source_audio_seconds,
        wall = a?.session_wall_seconds;
      if (produced === undefined || wall === undefined) return;
      if (lastAudio && produced < lastAudio.produced) lastAudio = undefined;
      if (lastAudio && wall > lastAudio.wall) setRate((produced - lastAudio.produced) / (wall - lastAudio.wall));
      lastAudio = { produced, wall };
    } catch {
      /* a replaced session has no audio status */
    }
  };

  /** The heartbeat, once a second: a stalled main loop shows nowhere else. */
  let readingHealth = false;
  const pollHealth = async () => {
    setNow(Date.now());
    if (readingHealth) return;
    readingHealth = true;
    const history = utilization();
    try {
      const s = await runtime()?.controllerStatus?.();
      setLoad(s?.load);
      history.emulator(s?.load);
      if (s?.heartbeat) watchHealth({ stalled: s.heartbeat.stalled, stallMs: s.heartbeat.stall_ms });
      setControllerJob(s?.job ?? null);
      syncControllerJob(s?.job, () => void runtime()?.cancelJob?.());
    } catch {
      setLoad(undefined);
      history.emulator(null);
    } finally {
      readingHealth = false;
    }
  };

  // an error banner goes away by itself after 8 s, unless the message changed meanwhile
  createEffect(() => {
    const text = error();
    if (!text) return;
    const timer = window.setTimeout(() => setError((current) => (current === text ? '' : current)), 8000);
    onCleanup(() => window.clearTimeout(timer));
  });

  const [toolDrawer, setToolDrawer] = createSignal<'Analyzer' | 'Performance'>();
  createEffect(() => {
    if (logOpen()) setToolDrawer(undefined);
  });
  const [performanceView, setPerformanceView] = createSignal<PerformanceView>('Utilization');
  const [spanRecording, setSpanRecording] = createSignal<SpanRecording>();
  const analyzerOpen = () => toolDrawer() === 'Analyzer';
  const drawerName = () => (logOpen() ? 'Log' : toolDrawer());
  const drawerOpen = () => !!drawerName();
  const toggleLog = () =>
    batch(() => {
      setLogOpen(!logOpen());
      setToolDrawer(undefined);
    });
  const toggleTool = (tool: 'Analyzer' | 'Performance') =>
    batch(() => {
      setLogOpen(false);
      setToolDrawer((open) => (open === tool ? undefined : tool));
    });
  const showMetrics = () => {
    setPage('run');
    setInspectorTab('metrics');
    setInspectorOpen(true);
    requestAnimationFrame(() => document.getElementById('side-tab-metrics')?.focus());
  };
  const dockPx = () => startDock(dockSaved(), windowPx());
  const dockDragTo = (clientY: number) => {
    if (!dockEl) return;
    setDockSaved(dockAt(clientY, dockEl.getBoundingClientRect().bottom, windowPx()));
  };
  const dockNudge = (delta: number) => {
    setDockSaved(clampDock(dockPx() + delta, windowPx()));
    writeDockPref(dockSaved());
  };
  const onResize = () => setWindowPx(window.innerHeight);
  const onKey = (e: KeyboardEvent) => {
    // ⌘J / Ctrl+J opens and closes the Log.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'j') {
      e.preventDefault();
      toggleLog();
      return;
    }
    // ⌘R / Ctrl+R reloads the firmware file, not the page; with Shift it reloads the modules into the running session.
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      if (!isEmulator() || app.target.switching() || busy('emulator')) return;
      if (e.shiftKey) void reloadModules();
      else void reload('Reloaded at');
    }
  };
  onMount(() => {
    void createNativeRuntime(props.host, update).then((r) => {
      if (!r) {
        setOutsideApp(true);
        return;
      }
      setRuntime(r);
      void props.host.emu
        .buildInfo<BuildInfo>()
        .then(setBuildInfo)
        .catch(() => setBuildInfo(undefined));
      void (async () => {
        const launch = await props.host.emu.launch();
        if (launch.loaded) await pollSource();
        // The launcher's `--no-dsp` / `--dsp-on` wins over the saved choice for this session, also for the first load below; only a
        // click on the tick saves a choice. The controller reports the flag before any firmware is loaded.
        await refreshInfo();
        const dsp = startupDsp(bootDsp(), info()?.dsp);
        if (dsp !== bootDsp()) setBootDsp(dsp);
        if (launch.loaded) {
          if (dspIsPending()) await reload(`Started with DSP ${dsp ? 'on' : 'off'} at`);
          return;
        }
        // The app opens empty; a command-line file, else the last opened file when "Load on start" is ticked and the target is the emulator,
        // boots now. With a device as target the last file waits for the first visit of the Emulator page (the effect below).
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
      })();
      timers.push(window.setInterval(() => void pollSource(), 1000));
      timers.push(window.setInterval(() => void pollAudio(), 1000));
      timers.push(window.setInterval(() => void pollHealth(), 1000));
    });
    window.addEventListener('keydown', onKey);
    window.addEventListener('resize', onResize);
  });
  onCleanup(() => {
    timers.forEach((t) => window.clearInterval(t));
    window.removeEventListener('keydown', onKey);
    window.removeEventListener('resize', onResize);
    void runtime()?.stop();
    runtime()?.dispose();
  });

  const setEmulatorPaused = (pause: boolean) => {
    const r = runtime();
    if (!r) return;
    if (pause) r.pause();
    else r.resume();
    setPaused(pause);
    logEvent('emulator', 'info', paused() ? 'paused' : 'resumed');
  };
  const restart = async () => {
    // a boot or reset of the emulator: the indicator asks again
    if (target().kind === 'emulator') noteConnection({ type: 'target' });
    health = healthStart();
    const began = performance.now();
    try {
      const s = await runtime()?.restart();
      if (s) setEmulatorGeneration((n) => n + 1);
      if (s) update({ type: 'snapshot', snapshot: s });
      setPaused(false);
      await refreshInfo();
      if (target().kind === 'emulator' && emulatorRuntime() === 'no')
        noteConnection({ type: 'noruntime', reason: EMULATOR_NO_RUNTIME });
      logEvent('reset', 'info', `emulator reset · ${duration(performance.now() - began)}`);
      bumpCardRefresh();
      window.setTimeout(bumpCardRefresh, 2500);
    } catch (e) {
      setError(message(e));
      logInput(failureEntry('reset', 'reset', message(e)));
    }
  };
  const runControl = async (op: 'pause' | 'resume' | 'reset') => {
    try {
      await selected().run[op]?.();
    } catch (error) {
      setError(message(error));
    }
  };
  const saveDiagnostics = async () => {
    try {
      const r = runtime();
      const report = await r?.diagnostics();
      if (!r?.saveDiagnostics || !report) return;
      const path = await r.saveDiagnostics(
        `digiemu-${report.device}-diagnostics.json`,
        JSON.stringify(report, null, 2),
      );
      setReloadNote(`Diagnostics saved to ${path}`);
      logEvent('emulator', 'info', `diagnostics saved to ${path}`);
    } catch (e) {
      setError(`Diagnostics failed: ${message(e)}`);
      logInput(failureEntry('emulator', 'diagnostics', message(e)));
    }
  };
  const chooseSamples = async (list: FileList | null) => {
    const cards = runtime()?.cards;
    if (!cards || !list) return;
    const files = folderFiles(Array.from(list));
    if (folderInput) folderInput.value = '';
    if (files.length === 0) {
      setCard({ state: 'error', message: 'No files in that folder.' });
      logEvent('card', 'error', 'sample folder: no files in that folder');
      return;
    }
    try {
      await runJob(
        'Loading the sample card',
        { group: 'emulator', cancel: () => void runtime()?.cancelJob?.() },
        async () => {
          await startCard(cards, files, (done, total) =>
            setCard({ state: 'working', message: `Sending samples (${done}/${total})`, fraction: 0 }),
          );
          while (true) {
            const s = await cards.status();
            setCard(s);
            if (s.state !== 'working') {
              logEvent('card', s.state === 'error' ? 'error' : 'info', cardLine(s));
              break;
            }
            await new Promise((r) => setTimeout(r, 400));
          }
        },
      );
    } catch (e) {
      if (e instanceof JobBusyError) return;
      if (isCancelled(e)) {
        setReloadNote('Sample card load cancelled');
        logEvent('card', 'info', 'sample folder: cancelled');
        return;
      }
      setCard({ state: 'error', message: message(e) });
      logInput(failureEntry('card', 'sample folder', message(e)));
    }
  };

  // the Log drawer
  const badge = () => (logOpen() ? 0 : unseenErrors(logState().entries, seenId()));
  const mac = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform);
  const logKey = mac ? '⌘J' : 'Ctrl+J';
  const reloadKey = mac ? '⌘R' : 'Ctrl+R';
  const modulesKey = mac ? '⇧⌘R' : 'Ctrl+Shift+R';
  // an open drawer has seen everything logged so far
  createEffect(() => {
    const entries = logState().entries;
    if (logOpen()) setSeenId(entries.at(-1)?.id ?? 0);
  });
  // the runtime logs of the emulator and of the device on the MIDI port chosen last (nothing is read while the emulator is paused)
  const dkprof = createDkProfLoader(props.host);
  const logFollow = createLogFollow({
    follow: followOn,
    flashing: () => flashRunning(flashRun()),
    shown: windowShown,
    drawerOpen: logOpen,
    emulator: props.logSources?.emulator ?? {
      running: () => !!status() && !paused(),
      session: async () => followSession(await emulatorSession()),
      makeNames: () => createEmulatorNames(props.host, dkprof),
    },
    device: props.logSources?.device ?? {
      port: lastMidiPort,
      session: async () => followSession(await lastMidiSession()),
      present: async () => (await app.env.targets.ports()).includes(lastMidiPort() ?? ''),
      makeNames: () => createDeviceNames(props.host, dkprof),
    },
  });

  const fileLabel = () => source()?.path || 'Open firmware…';
  /** The last three parts of the path; the full path is the tooltip. */
  const shortPath = () => {
    const parts = fileLabel().split('/');
    return parts.length > 3 ? `…/${parts.slice(-3).join('/')}` : fileLabel();
  };
  const phase = () => {
    const s = status();
    if (!s) return '';
    if (s.frame_revision === 0) return 'booting';
    return s.frame_source === 'intro' ? 'startup display' : s.phase;
  };
  const sink = () => audio()?.sink ?? undefined;

  /** The Metrics tab of the right-hand column: what the emulator measures, polled once a second, and two test actions. */
  const metrics = (
    <div class="card-body metrics">
      <p class="poll">1 s poll</p>
      <Show when={!status()}>
        <p class="note">The emulator is not running: no readings yet.</p>
      </Show>
      <dl>
        <dt>Firmware</dt>
        <dd>{status() ? `${status()!.device} ${status()!.version} · ${phase()}` : '—'}</dd>
        <dt>Instructions</dt>
        <dd>{status()?.icount.toLocaleString() ?? '—'}</dd>
        <dt>DSP</dt>
        <dd>{dspLine(info()?.dsp)}</dd>
        <dt>Audio output</dt>
        <dd>{audio()?.output_device ?? (audio()?.setup_needed ? 'no audio for this firmware' : '—')}</dd>
        <dt>Speed</dt>
        <dd>{rate() !== undefined ? `${rate()!.toFixed(2)}× real time (recent)` : '—'}</dd>
        <dt>Underruns</dt>
        <dd classList={{ warn: (sink()?.underrun_events ?? 0) > 0 }}>{sink()?.underrun_events ?? 0}</dd>
        <dt>DSP cost</dt>
        <dd>
          {typeof audio()?.dsp_tiers?.dsp_cpu_per_audio_s === 'number'
            ? `${audio()!.dsp_tiers!.dsp_cpu_per_audio_s!.toFixed(2)} CPU s / audio s`
            : '—'}
        </dd>
        <dt>Produced</dt>
        <dd>{audio() ? `${audio()!.source_audio_seconds.toFixed(2)} s` : '—'}</dd>
        <dt>CF load</dt>
        <dd>{load_() ? `${Math.round(load_()!.cf.real_pct)} %` : '—'}</dd>
        <dt title="Estimate for the real chip. The model figure counts one cycle per instruction, as the on-device HEALTH page does in the emulator.">
          DSP load (est.)
        </dt>
        <dd>
          {load_()?.dsp
            ? `${Math.round(load_()!.dsp!.real_pct)} % est. (worst ${Math.round(load_()!.dsp!.real_high_pct)} %), model ${Math.round(load_()!.dsp!.model_pct)} %`
            : '—'}
        </dd>
        <dt title="Largest estimated DSP load of any window since the meter was reset">DSP window peak</dt>
        <dd>{load_()?.dsp?.peak_pct !== undefined ? `${Math.round(load_()!.dsp!.peak_pct!)} % est.` : '—'}</dd>
      </dl>
      <Show when={reloadNote()}>
        <p class="note">{reloadNote()}</p>
      </Show>
      <Show when={card()}>
        <p class="note">{cardLine(card())}</p>
      </Show>
      <Show
        when={
          registry.ids.includes(device()) && registry.get(device()).capabilities.card === 'path-fs' && runtime()?.cards
        }
      >
        <div class="acts">
          <input
            ref={(el) => {
              folderInput = el;
              el.setAttribute('webkitdirectory', '');
            }}
            type="file"
            multiple
            hidden
            onChange={(e) => void chooseSamples(e.currentTarget.files)}
          />
          <button
            type="button"
            class="tb"
            disabled={!status() || busy('emulator')}
            title="Pick a plus_drive-style folder: WAVs become samples (track N plays file N, sorted by name), .mmcfs/ and digikit/ are used as they are"
            onClick={() => folderInput?.click()}
          >
            Sample folder…
          </button>
        </div>
      </Show>
    </div>
  );

  return (
    <div
      class="console"
      classList={{ 'dock-dragging': dockDrag() }}
      style={{
        '--accent': panel().accent,
        '--func-legend': panel().func_legend,
        '--dock-h': drawerOpen() ? `${dockPx()}px` : '0px',
      }}
    >
      <TitleBar onError={(m) => setError(m)} />

      <Show when={error() || outsideApp()}>
        <p class="alert" role="alert">
          <span>
            {outsideApp() ? 'This page is the desktop app’s console; start it with mise run workbench.' : error()}
          </span>
          <Show when={error()}>
            <button type="button" class="dismiss" aria-label="Dismiss" onClick={() => setError('')}>
              ×
            </button>
          </Show>
        </p>
      </Show>

      <div class="shell">
        <nav class="sidebar" classList={{ closed: !sidebarOpen() }} aria-label="Pages">
          <button
            type="button"
            class="side toggle"
            aria-expanded={sidebarOpen()}
            aria-label={sidebarOpen() ? 'Collapse the sidebar' : 'Expand the sidebar'}
            title={sidebarOpen() ? 'Collapse the sidebar' : 'Expand the sidebar'}
            onClick={() => {
              setSidebarOpen(!sidebarOpen());
              writeSidebarPref(sidebarOpen());
            }}
          >
            <Icon d={sidebarOpen() ? 'M10 3L5 8L10 13' : 'M6 3L11 8L6 13'} />
          </button>
          <span class="side-group">Run</span>
          <button
            type="button"
            class="side"
            classList={{ on: page() === 'run' }}
            aria-current={page() === 'run' ? 'page' : undefined}
            title={isEmulator() ? 'The emulator' : 'Control the selected device'}
            onClick={() => setPage('run')}
          >
            <Icon d={isEmulator() ? PLAY : REMOTE} />
            <span>{isEmulator() ? 'Emulator' : 'Remote'}</span>
          </button>
          <span class="side-group">Device</span>
          <button
            type="button"
            class="side"
            classList={{ on: page() === 'transfer' }}
            aria-current={page() === 'transfer' ? 'page' : undefined}
            disabled={(!status() && target().kind === 'emulator') || (target().kind === 'emulator' && !runtimeHere())}
            title={
              target().kind === 'emulator' && !runtimeHere()
                ? noRuntimeTitle()
                : 'Choose the modules that run on the target (the emulator or a device on a MIDI port) and install them'
            }
            onClick={() => setPage('transfer')}
          >
            <Icon d={MODULES} />
            <span>Transfer</span>
          </button>
          <button
            type="button"
            class="side"
            classList={{ on: page() === 'files' }}
            aria-current={page() === 'files' ? 'page' : undefined}
            title="See every file in the target's card store: space, sizes and module headers"
            onClick={() => setPage('files')}
          >
            <Icon d={FOLDER} />
            <span>Files</span>
          </button>
          <button
            type="button"
            class="side"
            classList={{ on: page() === 'firmware' }}
            aria-current={page() === 'firmware' ? 'page' : undefined}
            title="Patch the stock firmware and flash it to a device over MIDI, or go back to stock"
            onClick={() => setPage('firmware')}
          >
            <Icon d={FIRMWARE} />
            <span>Firmware</span>
          </button>
          <button
            type="button"
            class="side settings-link"
            classList={{ on: page() === 'settings' }}
            aria-current={page() === 'settings' ? 'page' : undefined}
            aria-label="Settings"
            title="Settings"
            onClick={() => setPage('settings')}
          >
            <Icon d={SETTINGS} />
            <span>Settings</span>
          </button>
        </nav>
        <div class="content">
          {/* The faceplate stays mounted behind the Transfer and Remote pages: the emulator, its audio and the snapshot pump run on. */}
          <div class="emu-page" hidden={page() !== 'run'}>
            <header class="page-head emu-head" hidden={!isEmulator()}>
              <h2>Emulator</h2>
              <span class="file-chip" title={fileLabel()}>
                <button
                  type="button"
                  class="file-open"
                  disabled={busy('emulator')}
                  aria-label="Open firmware file"
                  title={source() ? 'Open another firmware file' : 'Open a firmware file'}
                  onClick={() => void openFirmware()}
                >
                  <Icon d={FOLDER} />
                  <span class="path">{shortPath()}</span>
                </button>
                <Show when={status()}>
                  <span class="tag device">
                    {status()!.device} {status()!.version}
                  </span>
                </Show>
                <Show when={source()}>
                  <button
                    type="button"
                    class="tag watch"
                    classList={{ off: !watching() }}
                    aria-pressed={watching()}
                    title="Reload when the file changes on disk"
                    onClick={() => setWatching(!watching())}
                  >
                    <i />
                    {watching() ? 'watching' : 'not watching'}
                  </button>
                </Show>
              </span>
              <span class="sp" />
              <label
                class="tag opt"
                title="Boot this firmware file when the app opens (a file named on the command line wins)"
              >
                <input
                  type="checkbox"
                  checked={loadOnStart()}
                  onChange={(e) => {
                    setLoadOnStart(e.currentTarget.checked);
                    writePref(LOAD_ON_START_KEY, e.currentTarget.checked ? 'on' : 'off');
                  }}
                />
                <span class="long">Load on start</span>
                <span class="short">On start</span>
              </label>
              <label
                class="tag opt"
                title={
                  runtimeHere()
                    ? 'Patch the digikit runtime into a stock firmware when it is opened or reloaded (the next Open or Reload)'
                    : noRuntimeTitle()
                }
              >
                <input
                  type="checkbox"
                  checked={bootRuntime() && runtimeHere()}
                  disabled={!runtimeHere()}
                  onChange={(e) => {
                    setBootRuntime(e.currentTarget.checked);
                    writeRuntimePref(e.currentTarget.checked);
                  }}
                />
                <span class="long">Boot with runtime</span>
                <span class="short">Runtime</span>
                <Show when={info()}>
                  {(i) => (
                    <span
                      class="rt"
                      classList={{ on: !!i().runtime?.patched, pending: runtimePending() }}
                      title={
                        runtimePending()
                          ? `The running image ${i().runtime ? 'has' : 'has no'} runtime; the tick applies at the next Reload (${reloadKey}) or Open`
                          : i().runtime?.patched
                            ? `${i().runtime?.modules?.length ?? 0} modules of the "${i().runtime?.set}" set, ${i().runtime?.cache_hit ? 'from the image cache' : 'patched now'}`
                            : i().runtime
                              ? 'The image has a patch record: it is loaded as it is'
                              : 'The image is loaded as it is'
                      }
                    >
                      {runtimePending()
                        ? 'on next Reload'
                        : i().runtime?.patched
                          ? `${i().runtime?.set}${i().runtime?.cache_hit ? ' (cached)' : ''}`
                          : i().runtime
                            ? 'patched image'
                            : 'stock'}
                    </span>
                  )}
                </Show>
              </label>
              <label
                class="tag opt"
                title={
                  dspHere()
                    ? 'Run the DSP (and a second CPU that runs its program) at the next Open or Reload. Off: ColdFire only, no DSP audio, a faster UI'
                    : `${deviceNameOf(device())} has no DSP and no second CPU`
                }
              >
                <input
                  type="checkbox"
                  checked={bootDsp() && dspHere()}
                  disabled={!dspHere()}
                  onChange={(e) => {
                    setBootDsp(e.currentTarget.checked);
                    writeDspPref(e.currentTarget.checked);
                  }}
                />
                <span class="long">DSP</span>
                <span class="short">DSP</span>
                <Show when={info()?.dsp}>
                  {(d) => (
                    <span
                      class="rt"
                      classList={{ on: d().enabled, pending: dspIsPending() }}
                      title={
                        dspIsPending()
                          ? `The running image has the DSP ${d().enabled ? 'on' : 'off'}; the tick applies at the next Reload (${reloadKey}) or Open`
                          : d().enabled
                            ? 'The running image has the DSP'
                            : 'The running image has no DSP'
                      }
                    >
                      {dspIsPending() ? 'on next Reload' : d().enabled ? 'on' : 'off'}
                    </span>
                  )}
                </Show>
              </label>
              <span class="tb-group">
                <DefaultModules
                  device={device()}
                  off={bootRuntime() ? '' : 'Boot with digikit runtime is off: no module can be installed'}
                  note="Uploaded and enabled in the emulator after the firmware loads."
                />
              </span>
              <span class="tb-group">
                <button
                  type="button"
                  class="tb"
                  classList={{ pending: runtimePending() }}
                  disabled={!source() || busy('emulator')}
                  aria-label="Reload"
                  title={`Reload the startup file (${reloadKey})${runtimePending() ? ' · applies the Runtime choice' : ''}`}
                  onClick={() => void reload('Reloaded at')}
                >
                  <Icon d={RELOAD} />
                  <span class="lbl">Reload</span>
                  <kbd class="lbl">{reloadKey}</kbd>
                </button>
                <button
                  type="button"
                  class="tb"
                  disabled={!status() || busy('emulator')}
                  aria-label="Reload modules"
                  title={`Build the stale module objects, then apply the modules into the running session, without a boot when only UI modules changed (${modulesKey}). A changed SHARC+ source still needs mise run dk:dev.`}
                  onClick={() => void reloadModules()}
                >
                  <Icon d={MODULES} />
                  <span class="lbl">Modules</span>
                  <kbd class="lbl">{modulesKey}</kbd>
                </button>
              </span>
              <span class="tb-group">
                <button
                  type="button"
                  class="tb icon"
                  disabled={!status() || app.target.switching() || !selected().run.pause}
                  aria-label={paused() ? 'Resume' : 'Pause'}
                  title={paused() ? 'Resume the emulator' : 'Pause the emulator'}
                  onClick={() => void runControl(paused() ? 'resume' : 'pause')}
                >
                  <Icon d={paused() ? PLAY : PAUSE} />
                </button>
                <button
                  type="button"
                  class="tb icon"
                  disabled={!status() || app.target.switching() || !selected().run.reset}
                  aria-label="Reset (emulator)"
                  title="Reset the running firmware to its ready state (the emulator's reset; not the device's RESTART command)"
                  onClick={() => void runControl('reset')}
                >
                  <Icon d={RESTART} />
                </button>
                <button
                  type="button"
                  class="tb icon"
                  disabled={!status()}
                  aria-label="Save diagnostics"
                  title="Save diagnostics"
                  onClick={() => void saveDiagnostics()}
                >
                  <Icon d={CHART} />
                </button>
              </span>
            </header>
            <div class="emu-row">
              <main
                hidden={!isEmulator()}
                class="stage"
                aria-label={`${panel().name} front panel`}
                style={{ visibility: booting() && !knownDevice() ? 'hidden' : undefined }}
              >
                <div class="surface">
                  <LivePanel
                    panel={panel()}
                    runtime={runtime}
                    onError={(e) => setError(e)}
                    ref={(api) => (live = api)}
                  />
                </div>
              </main>
              <Show when={sidePanelsShown() && !isEmulator() && !app.target.switching() && selected()} keyed>
                {(source) => <RemotePage source={source} />}
              </Show>
              <Show when={!app.target.switching() && selected()} keyed>
                {(source) => (
                  <SidePanels
                    host={props.host}
                    visible={sidePanelsShown()}
                    liveDsp={toolDrawer() === 'Performance' && performanceView() === 'DSP blocks'}
                    collectSpans={
                      toolDrawer() === 'Performance' &&
                      performanceView() !== 'Utilization' &&
                      source.target.kind !== 'none'
                    }
                    profileGeneration={isEmulator() ? emulatorGeneration() : 1}
                    onSpanRecording={setSpanRecording}
                    active={!isEmulator() || !!status()}
                    source={source}
                    metrics={
                      isEmulator() ? (
                        metrics
                      ) : (
                        <div class="card-body metrics">
                          <dl>
                            <dt>CF load</dt>
                            <dd>{percent(remoteHealth()?.cpu)}</dd>
                            <dt>DSP load</dt>
                            <dd>{percent(remoteHealth()?.dsp)}</dd>
                            <dt>DSP peak</dt>
                            <dd>{percent(remoteHealth()?.peak)}</dd>
                            <dt>Last sample received</dt>
                            <dd>{remoteHealth() ? new Date(remoteHealth()!.received).toLocaleTimeString() : '—'}</dd>
                          </dl>
                        </div>
                      )
                    }
                  />
                )}
              </Show>
            </div>
          </div>
          <Show when={page() === 'transfer'}>
            <TransferPage onFlash={() => setPage('firmware')} />
          </Show>
          <Show when={page() === 'files' && !app.target.switching() && selected()} keyed>
            {(ops) => (
              <FilesPage
                store={props.files?.store ?? ops.store}
                library={props.files?.library}
                refresh={cardRefresh}
                halted={() =>
                  target().kind === 'emulator' && paused()
                    ? 'The emulator is paused. Resume it to read its card.'
                    : null
                }
                save={(name, bytes) => props.host.fs.saveLog(bytes, name, 'file')}
                extra={
                  <Show when={target().kind === 'emulator'}>
                    <CardPanel host={props.host} reload={restart} />
                  </Show>
                }
              />
            )}
          </Show>
          <Show when={page() === 'firmware'}>
            <FirmwarePage />
          </Show>
          <main class="appearance-page" hidden={page() !== 'settings'} aria-label="Settings">
            <section>
              <h2>Settings</h2>
              <h3>Appearance</h3>
              <label>
                Theme
                <select
                  value={props.theme.current()}
                  onChange={(event) => props.theme.select(event.currentTarget.value)}
                >
                  <For each={Object.entries(themes)}>{([id, label]) => <option value={id}>{label}</option>}</For>
                </select>
              </label>
            </section>
            <Show when={props.host.symbols} keyed>
              {(symbols) => (
                <FirmwareSymbols
                  shown={page() === 'settings'}
                  symbols={symbols}
                  source={symbolSource()}
                  imageRevision={isEmulator() ? info() : connectionState()}
                  imageFor={(binding) => selectedSymbolImage(props.host, binding)}
                  targetName={targetLabel(target())}
                />
              )}
            </Show>
          </main>
        </div>
      </div>

      <Show when={drawerOpen()}>
        <section
          class="workbench-drawer"
          classList={{ 'log-drawer': logOpen() }}
          aria-label={drawerName()}
          ref={dockEl}
          style={{ height: `${dockPx()}px` }}
        >
          <div
            class="dock-handle"
            role="separator"
            aria-orientation="horizontal"
            aria-label={`Resize the ${drawerName()}`}
            aria-valuemin={DOCK_MIN_PX}
            aria-valuemax={dockMax(windowPx())}
            aria-valuenow={dockPx()}
            tabIndex={0}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              e.currentTarget.setPointerCapture(e.pointerId);
              setDockDrag(true);
            }}
            onPointerMove={(e) => dockDrag() && dockDragTo(e.clientY)}
            onPointerUp={() => {
              if (dockDrag()) writeDockPref(dockSaved());
              setDockDrag(false);
            }}
            onPointerCancel={() => setDockDrag(false)}
            onDblClick={() => {
              setDockSaved(undefined);
              writeDockPref(undefined);
            }}
            onKeyDown={(e) => {
              if (e.key === 'ArrowUp') dockNudge(DOCK_KEY_STEP);
              else if (e.key === 'ArrowDown') dockNudge(-DOCK_KEY_STEP);
              else return;
              e.preventDefault();
            }}
          />
          <Show
            when={logOpen()}
            fallback={
              <Show
                when={toolDrawer() === 'Performance'}
                fallback={
                  <Show when={props.host.analyzer}>
                    {(host) => (
                      <Suspense fallback={<p role="status">Opening analyzer…</p>}>
                        <AnalyzerPanel host={host()} target={selected().target} workers={props.analyzerWorkers} />
                      </Suspense>
                    )}
                  </Show>
                }
              >
                <Suspense fallback={<p role="status">Opening performance history…</p>}>
                  <PerformancePanel
                    points={utilization().points()}
                    now={now()}
                    estimated={isEmulator()}
                    view={performanceView()}
                    setView={setPerformanceView}
                    recording={spanRecording()}
                    activeTarget={selected().target.kind !== 'none' && (!isEmulator() || !!status())}
                  />
                </Suspense>
              </Show>
            }
          >
            <LogPanel
              host={props.host}
              follow={logFollow}
              emulatorRunning={props.logSources?.emulator.running ?? (() => !!status())}
              devicePort={props.logSources?.device.port ?? lastMidiPort}
              toggleKey={logKey}
            />
          </Show>
        </section>
      </Show>

      <footer class="statusbar">
        <div class="status-readings">
          <span>
            <i
              class="dot"
              classList={{
                paused: isEmulator() ? paused() || !status() || !!booting() : connection().state !== 'connected',
              }}
            />
            {isEmulator()
              ? booting()
                ? bootingStatus(booting()!)
                : !status()
                  ? 'no firmware'
                  : paused()
                    ? 'paused'
                    : phase() || 'running'
              : connection().label}
          </span>
          <Show when={props.host.ports.audio}>{(host) => <RemoteAudio host={host()} source={selected} />}</Show>
          <Show when={isEmulator() && audio()?.output_device}>
            <span>
              <Icon d={SPEAKER} />
              {audio()!.output_device}
            </span>
          </Show>
          <Show when={isEmulator() && rate() !== undefined}>
            <span title="Emulated audio seconds per wall second">
              {rate()!.toFixed(2)}× RT
              <Meter value={rate()! * 100} tone="amber" />
            </span>
          </Show>
          <Show when={isEmulator() && pgoWarning(buildInfo())}>
            {(w) => (
              <span class="warn pgo" title={w().title}>
                <Icon d={WARNING} />
                {w().label}
              </span>
            )}
          </Show>
          <Show when={isEmulator() && sink()}>
            <span classList={{ warn: (sink()?.underrun_events ?? 0) > 0 }}>underruns {sink()!.underrun_events}</span>
          </Show>
          <Show when={isEmulator() && load_()}>
            <span title="Estimated load of the real chips, from the manuals' cycle tables">
              CF {Math.round(load_()!.cf.real_pct)}%
              <Meter value={load_()!.cf.real_pct} />
            </span>
            <Show when={load_()!.dsp}>
              <span title="Estimated DSP load of the real chip. Model figure (as the HEALTH page shows in the emulator) and window peak are in the Metrics tab">
                DSP est. {Math.round(load_()!.dsp!.real_pct)}%
                <Meter value={load_()!.dsp!.real_pct} worst={load_()!.dsp!.real_high_pct} />
              </span>
            </Show>
          </Show>
          <Show when={!isEmulator()}>
            <span>CF {percent(remoteHealth()?.cpu)}</span>
            <span>DSP {percent(remoteHealth()?.dsp)}</span>
          </Show>
          <span class="sp" />
          <Show
            when={currentJob()}
            fallback={
              <Show when={isEmulator() && reloadNote()}>
                <span class="muted" title={reloadNote()}>
                  {reloadNote()}
                </span>
              </Show>
            }
          >
            {(job) => (
              <span class="job" title={job().detail ? `${job().label}: ${job().detail}` : job().label}>
                <Icon d={RELOAD} />
                <span class="label">
                  {job().group === 'emulator' ? 'Emulator' : targetLabel(target())} · {job().label}
                </span>
                <Show when={job().detail}>
                  <span class="detail">{job().detail}</span>
                </Show>
                <span>{Math.max(0, Math.round((now() - job().started) / 1000))} s</span>
                <Show when={job().progress !== undefined}>
                  <Meter value={job().progress! * 100} tone="amber" />
                  <span>~{Math.round(job().progress! * 100)}%</span>
                </Show>
                <Show when={jobs().list.length > 1}>
                  <span class="more">+{jobs().list.length - 1}</span>
                </Show>
                <Show when={job().cancel}>
                  <button type="button" class="cancel" onClick={() => job().cancel?.()}>
                    Cancel
                  </button>
                </Show>
              </span>
            )}
          </Show>
        </div>
        <Show when={props.host.analyzer}>
          <button
            type="button"
            class="sb"
            aria-expanded={analyzerOpen()}
            title="Show the selected target’s audio analyzer"
            onClick={() => toggleTool('Analyzer')}
          >
            Analyzer <Icon d={analyzerOpen() ? DOWN : UP} />
          </button>
        </Show>
        <button
          type="button"
          class="sb"
          aria-expanded={toolDrawer() === 'Performance'}
          title="Show CF and DSP utilization history"
          onClick={() => toggleTool('Performance')}
        >
          Performance <Icon d={toolDrawer() === 'Performance' ? DOWN : UP} />
        </button>
        <button type="button" class="sb" aria-expanded={logOpen()} title={`Log (${logKey})`} onClick={toggleLog}>
          Log
          <Show when={badge() > 0}>
            <b class="badge" aria-label={`${badge()} new error${badge() === 1 ? '' : 's'}`}>
              {badge()}!
            </b>
          </Show>
          <Icon d={logOpen() ? DOWN : UP} />
        </button>
        <button type="button" class="sb" title="Show Metrics for the selected target" onClick={showMetrics}>
          Metrics
        </button>
      </footer>
    </div>
  );
}
