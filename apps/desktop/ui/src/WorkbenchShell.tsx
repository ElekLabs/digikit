import {
  For,
  Match,
  Show,
  Switch,
  lazy,
  Suspense,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  untrack,
} from 'solid-js';
import CopyErrorButton from '@digi/workbench-core/ui/components/CopyErrorButton.tsx';
import ModulePageView from '@digi/workbench-core/ui/module-ui/ModulePageView.tsx';
import { moduleFilePage, moduleFilePages } from './module-file-pages';
import { runtimeModulePages } from './runtime-module-pages';
import type { ModulePageSource } from '@digi/workbench-core/module-ui/module-page.ts';
import { targetKey } from '@digi/workbench-core/transfer/session-core.ts';
import LivePanel, { type LivePanelApi } from '@digi/workbench-core/ui/components/LivePanel.tsx';
import { defaultFaceplate, panels } from '@digi/workbench-core/data/faceplates.ts';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import type { NativeAudioStatus } from '@digi/workbench-core/runtime/runtime.ts';
import { bootingStatus } from './boot-core';
import { bumpCardRefresh, cardRefresh } from './card-state';
import CardPanel from './CardPanel';
import { connectionView } from '@digi/workbench-core/transfer/connection-core.ts';
import { targetLabel } from '@digi/workbench-core/transfer/target.ts';
import { createTargetHealth } from './target-health';
import type { SpanRecording } from './span-recording';
import type { PerformanceView } from './PerformancePanel';
import { createSavedTimeline } from './saved-timeline';
import { createUtilizationHistory } from './utilization-history';
import { percent } from './device-health';
import type { Page } from './page-core';
import { failureEntry, healthChanges, healthStart } from './log-core';
import { createWorkbenchEmulator } from './emulator/emulator';
import { themes, type createTheme } from './theme';
import { createLogFollow, followSession, type FollowEnv } from './log-follow';
import { createDkProfLoader } from './dkprof-env';
import { createDeviceNames, createEmulatorNames, selectedSymbolImage } from './log-names-backend';
import { windowShown } from './log-follow-core';
import type { DesktopApps } from './desktop-app';
import WorkbenchDrawer from './WorkbenchDrawer';
import LogPanel from './LogPanel';
import FirmwareSymbols from './FirmwareSymbols';
import FirmwareCatalog, { type CatalogSelection } from './FirmwareCatalog';
import Segmented from '@digi/workbench-core/ui/components/Segmented.tsx';
import type { Host } from '@digi/workbench-core/host/host.ts';
import SidePanels from './SidePanels';
import TitleBar from './TitleBar';
import { setInspectorOpen, setInspectorTab } from './panel-open';
import RemotePage from './remote/RemotePage';
import RemoteAudio from './remote/RemoteAudio';
import type { WorkerFactory } from '@digi/workbench-core/workers.ts';
import { useFirmwareApp, useTransferApp } from '@digi/workbench-core/transfer/app-context.ts';
import { dspLine } from './dsp-core';
import { running as flashRunning } from '@digi/workbench-core/firmware/firmware-core.ts';
import FirmwarePage from '@digi/workbench-core/ui/firmware/FirmwarePage.tsx';
import FirmwareToolbar from './emulator/FirmwareToolbar';
import Navigation from './shell/Navigation';
import StatusBar from './shell/StatusBar';
import TransferPage from '@digi/workbench-core/ui/transfer/TransferPage.tsx';
import { targetOps } from '@digi/workbench-core/target-ops/session-store.ts';
import FilesPage from '@digi/workbench-core/ui/files/FilesPage.tsx';
import { cardLine, folderFiles, startCard, type CardStatus } from '@digi/workbench-core/runtime/cards.ts';
import type { ControllerJob } from '@digi/workbench-core/jobs/jobs-core.ts';
import { isCancelled } from '@digi/workbench-core/modules/backend.ts';
import { JobBusyError } from '@digi/workbench-core/jobs/jobs.ts';

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

const PerformancePanel = lazy(() => import('./PerformancePanel'));
const AnalyzerPanel = lazy(() => import('./analyzer/AnalyzerPanel'));

export interface WorkbenchShellOptions {
  modulePages?: ModulePageSource;
  initialPage?: Page;
  browserPreview?: boolean;
  logSources?: Pick<FollowEnv, 'emulator' | 'device'>;
  initialDockHeight?: number;
}

export default function WorkbenchShell(
  props: WorkbenchShellOptions & {
    theme: ReturnType<typeof createTheme>;
    host: Host;
    analyzerWorkers: WorkerFactory<'analyzer'>;
    tools: Pick<DesktopApps, 'drawer' | 'log' | 'logWriter' | 'emulatorActions'>;
  },
) {
  const app = useTransferApp();
  const fw = useFirmwareApp();
  const { drawer, log, logWriter, emulatorActions } = untrack(() => props.tools);
  const { logEvent, logInput } = logWriter;
  const logOpen = () => drawer.active() === 'Log';
  const savedTimeline = createSavedTimeline();
  const performanceOpen = () => drawer.active() === 'Performance';
  const toggleLog = () => drawer.toggle('Log');
  const { emulatorSession, lastMidiPort, lastMidiSession, target } = app.target;
  const { busy, currentJob, jobs, runJob, syncControllerJob } = app.jobs;
  const flashRun = fw.state.run;
  const [controllerJob, setControllerJob] = createSignal<ControllerJob | null>(null);
  const [error, setError] = createSignal('');
  const [reloadNote, setReloadNote] = createSignal('');
  const [now, setNow] = createSignal(Date.now());
  const [load_, setLoad] = createSignal<NativeAudioStatus['load']>();
  const [audio, setAudio] = createSignal<NativeAudioStatus>();
  const [rate, setRate] = createSignal<number>();
  const [dockDrag, setDockDrag] = createSignal(false);
  const [page, setPage] = createSignal<Page>(props.initialPage ?? 'run');
  const [moduleId, setModuleId] = createSignal('');
  const [firmwareTab, setFirmwareTab] = createSignal<'flash' | 'catalog'>('flash');
  const [catalogSelection, setCatalogSelection] = createSignal<CatalogSelection>();
  const [fromProfile, setFromProfile] = createSignal(false);
  function openCatalog(selection: CatalogSelection) {
    setCatalogSelection(selection);
    setFromProfile(selection.path !== undefined && selection.entry !== undefined);
    setFirmwareTab('catalog');
    setPage('firmware');
  }
  const filePages = moduleFilePages(app);
  const runtimePage = runtimeModulePages(app, (id, schema, open) => moduleFilePage(app, id, schema, open));
  const modulePages = createMemo(() => {
    const source = props.modulePages;
    if (app.target.switching() || app.target.libraryOnly()) return [];
    const key = targetKey(target());
    return app.inventory.modules().flatMap((module) => {
      const { name } = module;
      const view = source?.lookup(key, name) ?? runtimePage(module) ?? filePages.get(name);
      return view ? [{ id: name, name: view.descriptor.name, view }] : [];
    });
  });
  const modulePage = () => modulePages().find((item) => item.id === moduleId())?.view;
  createEffect(() => {
    if (page() === 'module' && !app.target.switching() && !modulePage()) setPage('transfer');
  });
  const [card, setCard] = createSignal<CardStatus>();
  let live: LivePanelApi | undefined;
  let folderInput: HTMLInputElement | undefined;
  let lastAudio: { produced: number; wall: number } | undefined;
  let health = healthStart();

  /** Log a guest fault, stall or DSP halt when it starts (the app polls the status; it gets no events from the controller). */
  const watchHealth = (reading: Parameters<typeof healthChanges>[1]) => {
    const seen = healthChanges(health, reading);
    health = seen.health;
    seen.entries.forEach(logInput);
  };

  const { runtime, outsideApp, buildInfo, signal, firmware } = createWorkbenchEmulator({
    host: untrack(() => props.host),
    actions: emulatorActions,
    pollAudio: () => pollAudio(),
    pollHealth: () => pollHealth(),
    storage: app.storage,
    jobs: app.jobs,
    target: app.target,
    connection: app.connection,
    systemPackage: fw.env.systemPackage && {
      read: fw.env.systemPackage,
      session: emulatorSession,
    },
    defaults: { tickedFor: fw.defaults.tickedFor, install: fw.env.defaultModules!.install },
    logWriter,
    page,
    showSnapshot: (snapshot) => live?.show(snapshot),
    releaseHeldKeys: () => live?.releaseAll(),
    setError,
    setReloadNote,
    watchSnapshotError: (error) => watchHealth({ error }),
    resetHealth: () => (health = healthStart()),
    controllerBusy: () => controllerJob() !== null,
    setBrowseDevice: app.state.setBrowseDevice,
    clearInventory: app.inventory.clear,
    bumpCardRefresh,
  });

  const {
    status,
    emulatorLoaded,
    paused,
    booting,
    knownDevice,
    info,
    emulatorGeneration,
    dspHere,
    reload,
    reloadModules,
    reset,
  } = firmware;

  // a memo: `status` is set once per display frame, and the panels must hear of a change of this value only
  const sidePanelsShown = createMemo(() => page() === 'run');
  const selected = createMemo(() =>
    targetOps(app.target, { pause: async () => firmware.pause(), resume: async () => firmware.resume(), reset }),
  );
  const isEmulator = () => selected().target.kind === 'emulator';
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
  // a memo, so its readers re-run only when the id changes
  const device = createMemo(() => {
    const d = knownDevice();
    return d !== undefined && Object.hasOwn(panels, d) ? d : defaultFaceplate;
  });
  const panel = () => panels[device()];
  const inventoryConnected = createMemo(() => connectionState() === 'connected');
  // Keep loaded modules available to the shell while Transfer is closed. Transfer's own
  // read publishes to the same inventory when it is open.
  createEffect(() => {
    const current = target();
    cardRefresh();
    inventoryConnected();
    if (
      page() === 'transfer' ||
      busy('target') ||
      app.target.libraryOnly() ||
      app.target.switching() ||
      current.kind === 'none' ||
      (current.kind === 'emulator' && paused()) ||
      untrack(connectionState) === 'noruntime'
    )
      return;
    const binding = app.target.bindSession();
    const read = app.inventory.reader();
    void binding
      .session()
      .then(async (s) => {
        const info = await s.info(true);
        binding.check();
        await read(s, info);
      })
      .catch(() => {});
  });
  const pollAudio = async () => {
    try {
      const a = await runtime()?.nativeAudio?.();
      if (signal.aborted) return;
      setAudio(a);
      watchHealth({ halted: a?.halted ?? null });
      const produced = a?.source_audio_seconds,
        wall = a?.session_wall_seconds;
      if (produced === undefined || wall === undefined || wall <= 0) {
        lastAudio = undefined;
        setRate(undefined);
        return;
      }
      if (!lastAudio || produced < lastAudio.produced || wall < lastAudio.wall) setRate(undefined);
      else if (wall > lastAudio.wall) setRate((produced - lastAudio.produced) / (wall - lastAudio.wall));
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
      if (signal.aborted) return;
      setLoad(s?.load);
      history.emulator(s?.load);
      if (s?.heartbeat) watchHealth({ stalled: s.heartbeat.stalled, stallMs: s.heartbeat.stall_ms });
      setControllerJob(s?.job ?? null);
      syncControllerJob(s?.job, () => void runtime()?.cancelJob?.());
    } catch {
      if (!signal.aborted) {
        setLoad(undefined);
        history.emulator(null);
      }
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

  const [performanceView, setPerformanceView] = createSignal<PerformanceView>('Utilization');
  const [spanRecording, setSpanRecording] = createSignal<SpanRecording>();
  const showMetrics = () => {
    setPage('run');
    setInspectorTab('metrics');
    setInspectorOpen(true);
    requestAnimationFrame(() => document.getElementById('side-tab-metrics')?.focus());
  };
  const onKey = (e: KeyboardEvent) => {
    // ⌘J / Ctrl+J opens and closes the Log.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'j') {
      e.preventDefault();
      toggleLog();
      return;
    }
    // ⌘R / Ctrl+R reloads the firmware file, not the page; with Shift it reloads the modules into the running session.
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'r') {
      if (props.browserPreview && (page() !== 'run' || !isEmulator())) return;
      e.preventDefault();
      if (!isEmulator() || app.target.switching() || busy('emulator')) return;
      if (e.shiftKey) void reloadModules();
      else void reload('Reloaded at');
    }
  };
  onMount(() => window.addEventListener('keydown', onKey));
  onCleanup(() => window.removeEventListener('keydown', onKey));

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

  const mac = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform);
  const logKey = mac ? '⌘J' : 'Ctrl+J';
  const reloadKey = mac ? '⌘R' : 'Ctrl+R';
  const modulesKey = mac ? '⇧⌘R' : 'Ctrl+Shift+R';
  // the runtime logs of the emulator and of the device on the MIDI port chosen last (nothing is read while the emulator is paused)
  const dkprof = createDkProfLoader(props.host);
  const logFollow = createLogFollow({
    follow: log.followOn,
    push: logWriter.logInputs,
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

  const phase = () => {
    const s = status();
    if (!s) return '';
    if (s.frame_revision === 0) return 'booting';
    return s.frame_source === 'intro' ? 'startup display' : s.phase;
  };
  const sink = () => audio()?.sink ?? undefined;
  const statusLabel = () => {
    if (!isEmulator()) return connection().label;
    const boot = booting();
    if (boot) return bootingStatus(boot);
    if (!status()) return 'no firmware';
    return paused() ? 'paused' : phase() || 'running';
  };

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

  const alertText = () =>
    outsideApp() ? 'This page is the desktop app’s console; start it with mise run workbench.' : error();

  return (
    <div
      class="console"
      classList={{ 'dock-dragging': dockDrag() }}
      style={{
        '--accent': panel().accent,
        '--func-legend': panel().func_legend,
      }}
    >
      <TitleBar onError={(m) => setError(m)} />

      <Show when={alertText()}>
        {(message) => (
          <p class="alert" role="alert">
            <span>{message()}</span>
            <CopyErrorButton class="tb" text={message()} />
            <Show when={error()}>
              <button type="button" class="dismiss" aria-label="Dismiss" onClick={() => setError('')}>
                ×
              </button>
            </Show>
          </p>
        )}
      </Show>

      <div class="shell">
        <Navigation
          storage={app.storage}
          page={page()}
          isEmulator={isEmulator()}
          modules={modulePages()}
          moduleId={moduleId()}
          onNavigate={setPage}
          onOpenModule={(id) => {
            setModuleId(id);
            setPage('module');
          }}
        />
        <div class="content">
          {/* The faceplate stays mounted behind the Transfer and Remote pages: the emulator, its audio and the snapshot pump run on. */}
          <div class="emu-page" hidden={page() !== 'run'}>
            <FirmwareToolbar
              firmware={firmware}
              hidden={!isEmulator()}
              device={device()}
              busy={busy('emulator')}
              reloadKey={reloadKey}
              modulesKey={modulesKey}
              canPause={!!status() && !app.target.switching() && !busy('emulator') && !!selected().run.pause}
              canReset={!!status() && !app.target.switching() && !busy('emulator') && !!selected().run.reset}
              onTogglePause={() => void runControl(paused() ? 'resume' : 'pause')}
              onReset={() => void runControl('reset')}
              onSaveDiagnostics={() => void saveDiagnostics()}
            />
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
                    liveDsp={performanceOpen() && performanceView() === 'DSP blocks'}
                    collectSpans={
                      performanceOpen() && performanceView() !== 'Utilization' && source.target.kind !== 'none'
                    }
                    profileGeneration={isEmulator() ? emulatorGeneration() : 1}
                    onSpanRecording={setSpanRecording}
                    openCatalog={
                      props.host.symbols?.catalogFrame
                        ? (path, sha256, entry) => openCatalog({ path, sha256, entry })
                        : undefined
                    }
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
          <Show when={page() === 'module' && modulePage()} keyed>
            {(view) => <ModulePageView page={view} current={() => modulePage() === view} />}
          </Show>
          <Show when={page() === 'transfer'}>
            <TransferPage
              onFlash={() => {
                setFirmwareTab('flash');
                setPage('firmware');
              }}
            />
          </Show>
          <Show when={page() === 'files' && !app.target.switching() && selected()} keyed>
            {(ops) => (
              <FilesPage
                store={ops.store}
                refresh={cardRefresh}
                halted={() =>
                  target().kind === 'emulator' && paused()
                    ? 'The emulator is paused. Resume it to read its card.'
                    : null
                }
                save={(name, bytes) => props.host.fs.saveLog(bytes, name, 'file')}
                extra={
                  <Show when={target().kind === 'emulator'}>
                    <CardPanel host={props.host} reload={reset} logWriter={logWriter} />
                  </Show>
                }
              />
            )}
          </Show>
          <Show when={page() === 'firmware'}>
            <div class="firmware-workspace">
              <Show when={props.host.symbols?.catalogFrame}>
                <div class="firmware-tabs">
                  <Segmented
                    label="Firmware view"
                    options={
                      [
                        { id: 'flash', label: 'Flash' },
                        { id: 'catalog', label: 'Catalog' },
                      ] as const
                    }
                    value={firmwareTab()}
                    onChange={setFirmwareTab}
                  />
                </div>
              </Show>
              <Show when={firmwareTab() === 'catalog' && props.host.symbols?.catalogFrame} fallback={<FirmwarePage />}>
                <FirmwareCatalog
                  symbols={props.host.symbols!}
                  selection={catalogSelection()}
                  source={symbolSource()}
                  imageRevision={isEmulator() ? info() : connectionState()}
                  imageFor={(source) => selectedSymbolImage(props.host, source)}
                  select={setCatalogSelection}
                  settings={() => setPage('settings')}
                  back={fromProfile() ? () => setPage('run') : undefined}
                />
              </Show>
            </div>
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
                  openCatalog={props.host.symbols?.catalogFrame ? (sha256) => openCatalog({ sha256 }) : undefined}
                />
              )}
            </Show>
          </main>
        </div>
      </div>

      <WorkbenchDrawer
        active={drawer.active()}
        storage={app.storage}
        initialHeight={props.initialDockHeight}
        onDragging={setDockDrag}
      >
        <Switch>
          <Match when={logOpen()}>
            <LogPanel
              host={props.host}
              follow={logFollow}
              log={log}
              close={() => drawer.select(undefined)}
              emulatorRunning={props.logSources?.emulator.running ?? (() => !!status())}
              devicePort={props.logSources?.device.port ?? lastMidiPort}
              toggleKey={logKey}
            />
          </Match>
          <Match when={performanceOpen()}>
            <Suspense fallback={<p role="status">Opening performance history…</p>}>
              <PerformancePanel
                points={utilization().points()}
                now={now()}
                estimated={isEmulator()}
                view={performanceView()}
                setView={setPerformanceView}
                recording={spanRecording()}
                savedTimeline={savedTimeline}
                activeTarget={selected().target.kind !== 'none' && (!isEmulator() || !!status())}
              />
            </Suspense>
          </Match>
          <Match when={drawer.active() === 'Analyzer' && props.host.analyzer}>
            {(host) => (
              <Suspense fallback={<p role="status">Opening analyzer…</p>}>
                <AnalyzerPanel host={host()} target={selected().target} workers={props.analyzerWorkers} />
              </Suspense>
            )}
          </Match>
        </Switch>
      </WorkbenchDrawer>

      <StatusBar
        isEmulator={isEmulator()}
        inactive={isEmulator() ? paused() || !status() || !!booting() : connection().state !== 'connected'}
        label={statusLabel()}
        remoteAudio={
          <Show when={props.host.ports.audio}>{(host) => <RemoteAudio host={host()} source={selected} />}</Show>
        }
        outputDevice={audio()?.output_device}
        dspOff={isEmulator() && !!status() && dspHere() && info()?.dsp?.enabled === false}
        rate={rate()}
        buildInfo={buildInfo()}
        underruns={sink()?.underrun_events}
        load={load_()}
        remoteHealth={remoteHealth()}
        currentJob={currentJob()}
        jobCount={jobs().list.length}
        now={now()}
        reloadNote={reloadNote()}
        targetName={targetLabel(target())}
        activeDrawer={drawer.active()}
        hasAnalyzer={!!props.host.analyzer}
        logKey={logKey}
        unreadErrors={log.unread()}
        onToggleDrawer={drawer.toggle}
        onShowMetrics={showMetrics}
      />
    </div>
  );
}
