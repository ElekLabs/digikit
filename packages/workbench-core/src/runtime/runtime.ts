import { createLinkBuffer, LinkServer } from '../sab-link.ts';
import { wasmModule, type WasmSource } from '../wasm-source';
import type { WorkerFactory, WorkerPort } from '../workers';
import type { DspStart } from '../dsp-worker.ts';
import type { DspStats, FastDefault, FastRegions } from '../split-session.ts';
import fastRegionTable from '../../../../config/emulator.json';
import { sha256Hex } from './local-inputs';
import type { CardBridge } from './cards';
import type { ReloadReport } from './reload';
import type { ControllerJob } from '../jobs/jobs-core';

const pcs = (list: string | undefined) =>
  (list ?? '')
    .split(',')
    .filter((x) => x.trim())
    .map((x) => parseInt(x.trim(), 16));

/**
 * The DSP fast tier of the page (dev page, `?fast=`). Without a query the DSP worker decides from the
 * core (a `FastDefault`): the `_contexts.browser` value (`4,2,1`) when the core is generic (`digi_sharc_aot_blocks() === 0`); otherwise
 * `desktop` when config/emulator.json has an entry for the firmware; otherwise off. `?fast=off` disables it; `desktop` is the desktop
 * configuration (the static regions and cluster roots of config/emulator.json for this
 * firmware); `auto` or `T,M,F` is run-time selection (threshold, min insns, follow); `desktop+auto`
 * both. A firmware without an entry in the table gets no static regions.
 */
export async function fastConfig(
  syx: Uint8Array,
  query = new URLSearchParams(location.search).get('fast'),
): Promise<[number, number, number] | FastRegions | FastDefault | undefined> {
  if (query === '' || query === 'off') return undefined;
  const table = fastRegionTable as Record<string, { fast_regions?: string; blocktrans?: string }>;
  const entry = table[await sha256Hex(syx)];
  // `m || 64` makes M=0 impossible.
  const auto = (text: string): [number, number, number] => {
    const [t, m, f] = text.split(',').map(Number);
    return [t || 256, m || 64, f || 0];
  };
  if (query === 'auto') return [256, 64, 0];
  if (query !== null && !query.startsWith('desktop')) return auto(query);
  const config: FastRegions = { regions: pcs(entry?.fast_regions), roots: pcs(entry?.blocktrans) };
  if (query === null) {
    // The generic-core default comes from `_contexts.browser` of the config file.
    const browser = (fastRegionTable as unknown as { _contexts?: Record<string, Record<string, string>> })._contexts
      ?.browser;
    const generic =
      browser?.when === 'generic-core' && browser.SHARC_FAST_AUTO ? auto(browser.SHARC_FAST_AUTO) : undefined;
    return { kind: 'default', desktop: entry ? config : undefined, generic };
  }
  if (query.startsWith('desktop+auto')) config.auto = [256, 64];
  return config;
}
export interface RuntimeStatus {
  device: string;
  version: string;
  icount: number;
  pc: number;
  ready: boolean;
  phase: string;
  error: string | null;
  frame_revision: number;
  frame_source: string | null;
  main_ui_reached: boolean;
  filesystem_verified: boolean | null;
  input_ready: boolean;
  input_pending: number;
  input_irqs: number;
  led_revision?: number;
}
/** `leds`: 64 panel LED values as 128 hex digits; `rgb`: 64 x (r,g,b) as 384 hex digits. Both only when `status.led_revision` changed. */
export interface RuntimeSnapshot {
  status: RuntimeStatus;
  frame?: Uint8Array;
  leds?: string;
  rgb?: string;
  polled?: boolean;
}
export interface RuntimeDiagnostics {
  schema_version: number;
  device: string;
  version: string;
  sharc_execution_connected: boolean;
  pcm_output_connected: boolean;
  [key: string]: unknown;
}
export interface NativeAudioStatus {
  sink_connected: boolean;
  playback_requested: boolean;
  flow_started: boolean;
  setup_needed?: boolean;
  source_audio_seconds: number;
  session_wall_seconds: number;
  output_device?: string;
  device_error: string | null;
  audio_seconds_per_session_wall_second?: number;
  audio_mode?: string;
  halted?: string | null;
  /** CPU meters of the last window, percent (docs/CONTROL.md `status.load`); the `real_*` values are estimates. */
  load?: {
    window_s: number;
    cf: { model_pct: number; real_pct: number; real_high_pct: number };
    dsp: {
      busy_per_block: number;
      model_pct: number;
      real_pct: number;
      real_high_pct: number;
      peak_pct?: number;
    } | null;
  } | null;
  dsp_tiers?: {
    worker_cpu_s: number;
    audio_s?: number;
    dsp_cpu_per_audio_s?: number | null;
    instructions: number;
    interpreter: number;
    fast_tier: number;
    aot_block: number;
  } | null;
  sink?: {
    stream_started: boolean;
    source_received_frames: number;
    queued_device_frames: number;
    underrun_events: number;
    error: string | null;
    rebuffering?: boolean;
    played_device_frames?: number;
    silent_device_frames?: number;
    stretch_rate?: number;
    recovery?: string;
    sink_capture?: string | null;
    events_total?: number;
  } | null;
}
/** Dev-only inputs of the coupled core: user-supplied, never bundled. */
export interface CoupledFiles {
  syx: Uint8Array;
  image: Uint8Array;
  dsp: Uint8Array;
  snapshot?: Uint8Array;
}
export interface CoupledStats {
  cf_icount?: number;
  frames: number;
  dsp_instructions: number;
  halted: string | null;
  nonzero_replies: number;
  missing_blocks: number;
  pcm_values: number;
  pcm_produced_seconds: number;
  production_elapsed_wall_seconds: number;
  pcm_seconds_per_wall_second: number;
  dsp_attached: boolean;
  pcm_port_connected: boolean;
  host: Record<string, unknown>;
  /** Two-worker session only: the DSP worker's busy time and the ColdFire worker's time blocked on it. */
  split?: { dsp_worker_busy_ms: number; cf_link_wait_ms: number; link_requests: number };
  /** Two-worker session with the fast tier: the kernel host's counters and `digi_sharc_fast_report`. */
  dsp_kernels?: { compiled: number; compileMs: number; wasmBytes: number; runs: number };
  dsp_fast?: unknown;
}
/** Native desktop only: `runtime` patches the digikit runtime into a stock image (the controller's `firmware.load {runtime}`). */
export interface LoadOptions {
  runtime?: boolean;
  /** Native desktop only: the card image of this load (`--card-image`). */
  card?: string;
}
/** The controller's report of the runtime patch of the running image: `patched` with the set and whether the image came from the
 * cache, or `patched: false` for an image that already had a patch record. `null`: stock, no runtime requested. */
export interface RuntimePatch {
  patched: boolean;
  set?: string;
  cache_hit?: boolean;
  modules?: string[];
  seconds?: number;
  reason?: string;
}
export interface FirmwareInfo {
  runtime: RuntimePatch | null;
}
/** The controller's heartbeat (`status.heartbeat`): whether the guest's main loop has stopped, and the stall threshold in emulated ms. */
export interface RuntimeHealth {
  stalled: boolean;
  stall_ms: number;
}
export interface EmulatorRuntime {
  /** Browser worker only, and only with a core built with the `sharc` feature. */
  loadCoupled?(files: CoupledFiles, audioPort: MessagePort): Promise<RuntimeSnapshot>;
  coupledAvailable?(): Promise<boolean>;
  coupledStats?(): Promise<CoupledStats>;
  /** Native desktop only: observational status for its default output sink. */
  nativeAudio?(): Promise<NativeAudioStatus | undefined>;
  /** Press a key, release it after `hold` guest instructions (emulation time, not wall time). */
  tap?(code: number, hold?: number): Promise<void>;
  diagnostics(): Promise<RuntimeDiagnostics>;
  /** Native desktop only: save a report to disk; returns the path written. */
  saveDiagnostics?(name: string, json: string): Promise<string>;
  /** Native desktop only (Digitakt II): the controller builds a card from the files and boots with it. */
  cards?: CardBridge;
  load(bytes: Uint8Array, name: string, options?: LoadOptions): Promise<RuntimeSnapshot>;
  /** Native desktop only: load a firmware file the app can read itself (the startup file), as `load` does. */
  loadPath?(path: string, options?: LoadOptions): Promise<RuntimeSnapshot>;
  /** Native desktop only: what the running image got from the digikit runtime patch (`status.firmware.runtime`). */
  firmwareInfo?(): Promise<FirmwareInfo | undefined>;
  /** Native desktop only: the controller's heartbeat and running job (one `status` call). */
  controllerStatus?(): Promise<{ heartbeat?: RuntimeHealth; job?: ControllerJob | null }>;
  /** Native desktop only: cancel the controller's long job (`job.cancel`); the cancelled call fails with error 10. */
  cancelJob?(): Promise<void>;
  /** The emulator's own reset of the running firmware (the browser worker boots the file again). Not the device RESTART command. */
  restart(): Promise<RuntimeSnapshot>;
  /** Native desktop only: `dev.reload`. The controller applies the modules again from the objects on disk and brings the running
   * session to them (a module swap into a restored main screen, or a boot of the new image). It never builds. */
  reloadModules?(): Promise<ReloadReport>;
  pause(): void;
  resume(): void;
  press(code: number): Promise<void>;
  release(code: number): Promise<void>;
  turn(encoder: number, detents: number): Promise<void>;
  stop(): Promise<void>;
  dispose(): void;
}
export type RuntimeUpdate = { type: 'snapshot'; snapshot: RuntimeSnapshot } | { type: 'error'; error: string };

export const drawFrame = (canvas: HTMLCanvasElement, frame: Uint8Array) => {
  if (frame.length !== 1024) return;
  const context = canvas.getContext('2d');
  if (!context) return;
  const pixels = context.createImageData(128, 64);
  for (let y = 0; y < 64; y += 1)
    for (let x = 0; x < 128; x += 1) {
      const on = (frame[7 - Math.floor(y / 8) + 8 * x] & (1 << (y % 8))) !== 0;
      const at = (y * 128 + x) * 4;
      pixels.data[at] = on ? 242 : 8;
      pixels.data[at + 1] = on ? 244 : 9;
      pixels.data[at + 2] = on ? 249 : 12;
      pixels.data[at + 3] = 255;
    }
  context.putImageData(pixels, 0, 0);
};

/** Two workers (ColdFire and DSP over a SharedArrayBuffer) need cross-origin isolation; `?split=0` keeps the single worker. */
export const splitAvailable = () =>
  typeof SharedArrayBuffer !== 'undefined' &&
  (globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated === true &&
  new URLSearchParams(location.search).get('split') !== '0';

/** What the browser runtime takes from the page: where its workers and its emulator core come from. */
export interface BrowserRuntimeDeps {
  workers: WorkerFactory;
  /** `emulator-core-sharc.wasm`, compiled once for the DSP worker of a split session. */
  wasm: WasmSource;
}

export function browserRuntime(onUpdate: (update: RuntimeUpdate) => void, deps: BrowserRuntimeDeps): EmulatorRuntime {
  const worker = deps.workers.create('runtime');
  let generation = 0,
    paused = true,
    source: Uint8Array | undefined,
    name = '';
  // Two-worker session: the DSP worker, its link and its latest statistics.
  let dsp: WorkerPort | undefined,
    link: SharedArrayBuffer | undefined,
    dspStats:
      | (DspStats & {
          pcm_produced_seconds: number;
          production_elapsed_wall_seconds: number;
          pcm_seconds_per_wall_second: number;
        })
      | undefined;
  let modulePromise: Promise<WebAssembly.Module> | undefined;
  const closeSplit = () => {
    if (link) new LinkServer(link).close();
    dsp?.terminate();
    dsp = undefined;
    link = undefined;
    dspStats = undefined;
  };
  const coreModule = () => (modulePromise ??= wasmModule(deps.wasm, 'emulator-core-sharc.wasm'));
  /** Starts the DSP worker; resolves with its link and the compiled core, or undefined when the core cannot be split. */
  const startDsp = async (files: CoupledFiles, audioPort: MessagePort) => {
    const module = await coreModule();
    if (!WebAssembly.Module.exports(module).some((e) => e.name === 'digi_dsp_reply')) return undefined;
    const fast = await fastConfig(files.syx);
    const buffer = createLinkBuffer(),
      thread = deps.workers.create('dsp');
    await new Promise<void>((resolve, reject) => {
      thread.onmessage = ({ data }) => {
        if (data.type === 'ready') resolve();
        else if (data.type === 'stats') dspStats = data;
        else if (data.type === 'error') {
          reject(new Error(data.error));
          onUpdate({ type: 'error', error: data.error });
        }
      };
      thread.onerror = (event) => reject(new Error(event.message));
      const start: DspStart = {
        type: 'start',
        module,
        image: files.image,
        state: files.dsp,
        link: buffer,
        fast,
        pcmPort: audioPort,
      };
      thread.postMessage(start, [audioPort]);
    }).catch((error) => {
      thread.terminate();
      throw error;
    });
    dsp = thread;
    link = buffer;
    return { buffer, module };
  };
  worker.onmessage = ({ data }: { data: RuntimeUpdate & { generation?: number } }) => {
    if (data.generation === generation) onUpdate(data);
  };
  const request = <T>(type: string, payload: Record<string, unknown> = {}) =>
    new Promise<T>((resolve, reject) => {
      const id = crypto.randomUUID();
      const listener = ({ data }: MessageEvent<{ reply?: string; value?: T; error?: string }>) => {
        if (data.reply !== id) return;
        worker.removeEventListener('message', listener);
        if (data.error) reject(new Error(data.error));
        else resolve(data.value as T);
      };
      worker.addEventListener('message', listener);
      worker.postMessage({ type, id, generation, ...payload });
    });
  return {
    diagnostics: () => request<RuntimeDiagnostics>('diagnostics'),
    async loadCoupled(files, audioPort) {
      generation += 1;
      paused = false;
      source = undefined;
      closeSplit();
      const copy = (bytes?: Uint8Array) => bytes?.slice().buffer;
      if (splitAvailable()) {
        const split = await startDsp(files, audioPort);
        if (split)
          return request<RuntimeSnapshot>('load-coupled', {
            bytes: copy(files.syx),
            snapshot: copy(files.snapshot),
            link: split.buffer,
            module: split.module,
          });
      }
      worker.postMessage({ type: 'audio-port', generation, port: audioPort }, [audioPort]);
      return request<RuntimeSnapshot>('load-coupled', {
        bytes: copy(files.syx),
        image: copy(files.image),
        dsp: copy(files.dsp),
        snapshot: copy(files.snapshot),
      });
    },
    async coupledStats() {
      const cf = await request<CoupledStats & { split?: { link_wait_ms: number; link_requests: number } }>(
        'coupled-stats',
      );
      if (!dsp || !cf.split) return cf;
      const d = dspStats,
        engine = (d?.engine ?? {}) as Partial<CoupledStats>;
      return {
        ...cf,
        frames: engine.frames ?? 0,
        dsp_instructions: engine.dsp_instructions ?? 0,
        halted: engine.halted ?? null,
        nonzero_replies: engine.nonzero_replies ?? 0,
        missing_blocks: engine.missing_blocks ?? 0,
        pcm_values: d?.pcmValues ?? 0,
        pcm_produced_seconds: d?.pcm_produced_seconds ?? 0,
        production_elapsed_wall_seconds: d?.production_elapsed_wall_seconds ?? 0,
        pcm_seconds_per_wall_second: d?.pcm_seconds_per_wall_second ?? 0,
        split: {
          dsp_worker_busy_ms: d?.busyMs ?? 0,
          cf_link_wait_ms: cf.split.link_wait_ms,
          link_requests: cf.split.link_requests,
        },
        dsp_kernels: d?.kernels,
        dsp_fast: d?.fast,
      };
    },
    coupledAvailable: () => request<boolean>('coupled-capable'),
    tap: (code, hold) => request<void>('tap', { code, hold }),
    async load(bytes, selectedName) {
      closeSplit();
      generation += 1;
      paused = false;
      source = bytes.slice();
      name = selectedName;
      return request<RuntimeSnapshot>('load', { bytes: bytes.buffer, name: selectedName });
    },
    async restart() {
      if (!source) throw new Error('No firmware selected');
      closeSplit();
      generation += 1;
      paused = false;
      return request<RuntimeSnapshot>('load', { bytes: source.slice().buffer, name });
    },
    pause() {
      paused = true;
      worker.postMessage({ type: 'pause', generation });
    },
    resume() {
      if (!paused) return;
      paused = false;
      worker.postMessage({ type: 'resume', generation });
    },
    press: (code) => request<void>('button', { code, down: true }),
    release: (code) => request<void>('button', { code, down: false }),
    turn: (encoder, detents) => request<void>('turn', { encoder, detents }),
    async stop() {
      closeSplit();
      generation += 1;
      paused = true;
      source = undefined;
      worker.postMessage({ type: 'stop', generation });
    },
    dispose() {
      closeSplit();
      generation += 1;
      paused = true;
      worker.terminate();
    },
  };
}
