import { For, Show, createSignal, onCleanup, onMount } from 'solid-js';
import { panels } from '../data/faceplates';
import { deviceProfiles, localReadyFirmwareSha256, type DeviceId } from '../data/device-profiles';
import { browserRuntime, type EmulatorRuntime, type RuntimeUpdate } from '../runtime/runtime';
import { parseLeds } from '../runtime/leds';
import { PcmSink, resumeOnNextGesture } from '../runtime/audio';
import { fetchLocalInputs, localManifest, sha256Hex } from '../runtime/local-inputs';
import CoupledAudio, { startCoupled } from './CoupledAudio';
import LivePanel, { type LivePanelApi } from './LivePanel';
import PanelArt from './PanelArt';
import SyxFile from './SyxFile';
import { benchRequested, runBench } from '../runtime/bench';
import { devEnabled } from '../dev';
import { identify } from '../patch/identify';
import { takeHandoff } from '../patch/handoff';
import './faceplate.css';

function BenchStatus() {
  onMount(() => {
    void runBench();
  });
  return <output id="bench-status">bench mode: see window.__bench</output>;
}

/** The emulator page: the bench (dev only, tools/devtools/browser_bench.mjs) or the panel. */
export default function Faceplate(props: { runtime?: EmulatorRuntime } = {}) {
  return (
    <Show when={!(devEnabled() && benchRequested())} fallback={<BenchStatus />}>
      <Panel runtime={props.runtime} />
    </Show>
  );
}

function Panel(props: { runtime?: EmulatorRuntime }) {
  const dev = devEnabled();
  const [device, setDevice] = createSignal<'dt2' | 'dn2'>('dt2');
  // `?leds=<128 hex digits>` shows a fixed LED state without a running emulator (a styling aid; a loaded firmware replaces it).
  onMount(() => {
    const preview = dev ? new URLSearchParams(location.search).get('leds') : null;
    if (preview) {
      live?.previewLeds(parseLeds(preview));
      setStage('run');
    }
  });
  const [status, setStatus] = createSignal('Choose a firmware file (.syx) to start.');
  // The page walks through three stages: pick a device, choose its firmware, run it.
  const [stage, setStage] = createSignal<'pick' | 'firmware' | 'run'>('pick');
  const [running, setRunning] = createSignal<{ device: string; version: string }>();
  const [phase, setPhase] = createSignal('');
  const [fileName, setFileName] = createSignal('');
  const [dragging, setDragging] = createSignal(false);
  const [reportAvailable, setReportAvailable] = createSignal(false);
  const [paused, setPaused] = createSignal(true);
  const [loaded, setLoaded] = createSignal(false);
  const [browserAudioSetup, setBrowserAudioSetup] = createSignal(false);
  const [browserAudioReset, setBrowserAudioReset] = createSignal(0);
  const [chosenFirmware, setChosenFirmware] = createSignal<{ name: string; bytes: Uint8Array }>();
  const [adopted, setAdopted] = createSignal<{ sink: PcmSink; note: string }>();
  const [runtimeSignal, setRuntimeSignal] = createSignal<EmulatorRuntime | undefined>(undefined);
  const runtime = () => runtimeSignal() ?? activeRuntime ?? props.runtime;
  let activeRuntime: EmulatorRuntime | undefined;
  let runtimeReady: Promise<EmulatorRuntime | undefined> | undefined;
  let live: LivePanelApi | undefined;
  let firmwareFile: File | undefined;
  const panel = () => panels[device()];
  const clearCanvas = () => live?.clear();
  const update = (event: RuntimeUpdate) => {
    if (event.type === 'error') {
      setPaused(true);
      setLoaded(false);
      setStatus(event.error);
      return;
    }
    const snapshot = event.snapshot;
    setReportAvailable(true);
    live?.show(snapshot);
    const s = snapshot.status;
    if (s.error) {
      setPaused(true);
      setLoaded(false);
    } else if (!loaded()) {
      setLoaded(true);
      setPaused(false);
    }
    if (s.device === 'dt2' || s.device === 'dn2') setDevice(s.device);
    setRunning({ device: s.device, version: s.version });
    const display =
      s.frame_revision === 0
        ? 'Booting — waiting for display'
        : s.frame_source === 'intro'
          ? 'Booting — startup display'
          : s.phase;
    setPhase(s.error ? s.error : display);
    setStatus(
      `${s.device} ${s.version} — ${display}, ${s.icount.toLocaleString()} instructions${s.error ? `: ${s.error}` : ''}`,
    );
  };
  let disposed = false;
  onMount(() => {
    runtimeReady = Promise.resolve(props.runtime ?? browserRuntime(update)).then((selected) => {
      if (disposed) {
        selected.dispose();
        return undefined;
      }
      activeRuntime = selected;
      setRuntimeSignal(selected);
      setBrowserAudioSetup(!props.runtime);
      return selected;
    });
  });
  onCleanup(() => {
    disposed = true;
    live?.releaseAll();
    void activeRuntime?.stop().catch((error) => update({ type: 'error', error: String(error) }));
    activeRuntime?.dispose();
  });
  const choose = async (file?: File) => {
    if (!file) return;
    setStage('run');
    setFileName(file.name);
    setRunning(undefined);
    setPhase('');
    // Dev-only coupled path (Digitone II): create and resume the AudioContext before any await.
    const coupledSink = dev ? new PcmSink() : undefined;
    void coupledSink?.resume().catch(() => {});
    if (coupledSink) resumeOnNextGesture(coupledSink);
    let kept = false;
    live?.releaseAll();
    setBrowserAudioReset((value) => value + 1);
    void runtime()?.stop();
    clearCanvas();
    setReportAvailable(false);
    setLoaded(false);
    firmwareFile = file;
    setStatus(`Loading ${file.name}…`);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      if (firmwareFile !== file) return;
      setChosenFirmware({ name: file.name, bytes: bytes.slice() });
      const identity = identify(await sha256Hex(bytes));
      if (identity) setDevice(identity.device);
      const selected = await runtimeReady;
      if (firmwareFile !== file || !selected) return;
      let coupledError = '';
      if (coupledSink) {
        try {
          const manifest = await localManifest();
          if (manifest?.dspImage && manifest.dspState && (await sha256Hex(bytes)) === localReadyFirmwareSha256) {
            const inputs = await fetchLocalInputs();
            if (inputs) {
              const snapshot = await startCoupled({
                runtime: selected,
                firmware: { name: file.name, bytes },
                image: inputs.image,
                dsp: inputs.state,
                snapshot: inputs.snapshot,
                sink: coupledSink,
              });
              if (firmwareFile !== file) return;
              kept = true;
              setAdopted({ sink: coupledSink, note: `firmware: ${file.name} (local dev inputs)` });
              update({ type: 'snapshot', snapshot });
              setPaused(false);
              setLoaded(true);
              setStatus('Digitone II with audio (local dev inputs) — press NO, then Trig 1');
              return;
            }
          }
        } catch (error) {
          coupledError = String(error);
        }
        if (firmwareFile !== file) return;
      }
      const snapshot = await selected.load(bytes, file.name);
      if (firmwareFile !== file) return;
      update({ type: 'snapshot', snapshot });
      setPaused(false);
      setLoaded(true);
      if (coupledError) setStatus(`Local audio start failed (${coupledError}); booted without audio.`);
    } catch (error) {
      if (firmwareFile === file) {
        setPaused(true);
        setLoaded(false);
        setStatus(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (!kept) coupledSink?.close();
    }
  };
  // /patch hands a patched image over through IndexedDB (never the network): boot it as if it had been chosen.
  onMount(() => {
    if (new URLSearchParams(location.search).get('handoff') !== '1') return;
    setStage('run');
    void takeHandoff()
      .then((item) => {
        if (!item) {
          setStatus('Nothing to boot: open the patch page and press Boot in emulator.');
          return;
        }
        if (item.device === 'dt2' || item.device === 'dn2') setDevice(item.device);
        void choose(new File([item.bytes], item.name));
      })
      .catch((error) =>
        setStatus(`Could not read the patched firmware: ${error instanceof Error ? error.message : String(error)}`),
      );
  });
  const exportDiagnostics = async () => {
    try {
      const report = await runtime()?.diagnostics();
      if (!report) return;
      const name = `digiemu-${report.device}-diagnostics.json`,
        json = JSON.stringify(report, null, 2);
      const url = URL.createObjectURL(new Blob([json], { type: 'application/json' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = name;
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (error) {
      setStatus(`Diagnostic export failed: ${String(error)}`);
    }
  };
  const changeFirmware = () => {
    live?.releaseAll();
    setBrowserAudioReset((value) => value + 1);
    void runtime()?.stop();
    clearCanvas();
    firmwareFile = undefined;
    setReportAvailable(false);
    setLoaded(false);
    setPaused(true);
    setRunning(undefined);
    setPhase('');
    setStatus('Choose a firmware file (.syx) to start.');
    setStage('firmware');
  };
  const base = import.meta.env.BASE_URL.replace(/\/?$/, '/');
  const accepted = (d: DeviceId) => deviceProfiles[d].firmware.filter((f) => f.main_sha256).map((f) => f.version);
  const runningName = () => {
    const r = running();
    if (!r) return panel().name;
    const name = r.device === 'dt2' || r.device === 'dn2' ? panels[r.device].name : r.device;
    return `${name} · OS ${r.version}`;
  };
  const restart = async () => {
    try {
      const snapshot = await runtime()?.restart();
      if (snapshot) update({ type: 'snapshot', snapshot });
      setPaused(false);
    } catch (error) {
      setPaused(true);
      setLoaded(false);
      setStatus(String(error));
    }
  };
  return (
    <main
      class="emulator"
      classList={{ running: stage() === 'run' }}
      data-loaded={loaded() ? 'true' : 'false'}
      style={{ '--accent': panel().accent, '--func-legend': panel().func_legend }}
    >
      <input
        id="emu-file"
        class="visually-hidden"
        type="file"
        accept=".syx,application/octet-stream"
        onChange={(e) => {
          void choose(e.currentTarget.files?.[0]);
          e.currentTarget.value = '';
        }}
      />
      <Show when={stage() === 'pick'}>
        <div class="wrap emu-pick">
          <header>
            <h1>Run your firmware in the browser</h1>
            <p>Pick your device, then choose its firmware file: stock, or patched with digikit. Nothing is uploaded.</p>
          </header>
          <div class="devices">
            <For each={['dt2', 'dn2'] as const}>
              {(d) => (
                <button
                  type="button"
                  class={`devcard ${d}`}
                  style={{ '--accent': panels[d].accent, '--func-legend': panels[d].func_legend }}
                  onClick={() => {
                    setDevice(d);
                    setStage('firmware');
                  }}
                >
                  <span class="devcard-art" aria-hidden="true">
                    <PanelArt panel={panels[d]} />
                  </span>
                  <span class="devcard-meta">
                    <span>
                      <span class="devcard-name">{panels[d].name}</span>
                      <span class="accepts">
                        OS <For each={accepted(d)}>{(v) => <span class="ver">{v}</span>}</For>
                      </span>
                    </span>
                    <span class="go">
                      Choose
                      <svg viewBox="0 0 16 16" aria-hidden="true">
                        <path
                          d="M3 8H13M9 4L13 8L9 12"
                          fill="none"
                          stroke="currentColor"
                          stroke-width="2"
                          stroke-linecap="round"
                          stroke-linejoin="round"
                        />
                      </svg>
                    </span>
                  </span>
                </button>
              )}
            </For>
          </div>
        </div>
      </Show>
      <Show when={stage() === 'firmware'}>
        <div class="wrap emu-fw">
          <button type="button" class="back" onClick={() => setStage('pick')}>
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="M13 8H3M7 4L3 8L7 12"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            Change device
          </button>
          <h1>{panel().name} firmware</h1>
          <div
            class="emu-drop"
            classList={{ dragging: dragging() }}
            onDragOver={(e) => {
              e.preventDefault();
              setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(e) => {
              e.preventDefault();
              setDragging(false);
              void choose(e.dataTransfer?.files[0]);
            }}
          >
            <SyxFile />
            <strong>Drop a {panel().name} .syx here</strong>
            <span>
              OS {accepted(device()).join(' or ')}, stock or patched with digikit. Download stock firmware from
              Elektron's support site.
            </span>
            <label class="btn primary" for="emu-file">
              Choose file
            </label>
          </div>
          <p class="emu-alt">
            Want new machines first? <a href={`${base}firmware/`}>Patch your firmware</a>, then run it here.
          </p>
        </div>
      </Show>
      <Show when={stage() === 'run'}>
        <div class="wrap runbar-wrap">
          <div class="runbar">
            <span class="dot" classList={{ paused: paused() || !loaded() }} aria-hidden="true" />
            <div class="who">
              <strong>{runningName()}</strong>
              <small>
                {fileName()}
                {phase() ? ` · ${phase()}` : ''}
              </small>
            </div>
            <span class="sound-note" title="The browser emulator has no audio yet. The desktop app plays it.">
              No sound yet
            </span>
            <button
              type="button"
              class="btn sm quiet"
              disabled={!loaded()}
              onClick={() => {
                if (paused()) {
                  runtime()?.resume();
                  setPaused(false);
                } else {
                  runtime()?.pause();
                  setPaused(true);
                }
              }}
            >
              {paused() ? 'Resume' : 'Pause'}
            </button>
            <button type="button" class="btn sm quiet" disabled={!loaded()} onClick={() => void restart()}>
              Restart
            </button>
            <button type="button" class="btn sm" onClick={changeFirmware}>
              Change firmware
            </button>
          </div>
          <Show when={!loaded() && !status().startsWith('Loading') && !status().startsWith('Choose')}>
            <p class="emu-error" role="alert">
              {status()}
            </p>
          </Show>
        </div>
      </Show>
      <section class="panel-wrap" hidden={stage() !== 'run'} aria-label={`${panel().name} front panel`}>
        <LivePanel
          panel={panel()}
          runtime={runtime}
          onError={(error) => update({ type: 'error', error })}
          ref={(api) => (live = api)}
        />
      </section>
      <Show when={stage() === 'run'}>
        <div class="wrap emu-foot">
          <p class="hint">
            Click keys to press them. Drag or scroll knobs to turn them. Click FUNC, then a key, for a FUNC combination.
          </p>
          <Show when={dev}>
            <details class="devtools">
              <summary>Developer tools</summary>
              <div class="devtools-row">
                <output class="status" aria-live="polite">
                  {status()}
                </output>
                <Show when={dev}>
                  <button disabled={!reportAvailable()} onClick={() => void exportDiagnostics()}>
                    Export diagnostics
                  </button>
                </Show>
              </div>
              <Show when={dev && browserAudioSetup()}>
                <CoupledAudio
                  reset={browserAudioReset()}
                  firmware={chosenFirmware}
                  adopted={adopted}
                  runtime={() => runtimeReady ?? Promise.resolve(undefined)}
                  onStarted={(snapshot) => {
                    live?.releaseAll();
                    clearCanvas();
                    update({ type: 'snapshot', snapshot });
                    setPaused(false);
                    setLoaded(true);
                  }}
                  onError={(error) => setStatus(error)}
                />
              </Show>
            </details>
          </Show>
        </div>
      </Show>
    </main>
  );
}
