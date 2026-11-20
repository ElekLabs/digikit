/**
 * Headless throughput bench (dev server only): `/emulator/?bench=1&seconds=N[&warmup=S]`.
 * Loads the local coupled inputs, presses Trig 1 as the panel's Trig 1 button
 * does (`tap(25)`), runs the worker flat out until N audio seconds exist and
 * publishes the result on `window.__bench` and as a `BENCH_JSON` console line.
 * No AudioContext: the PCM port is drained and counted. Driven by
 * tools/devtools/browser_bench.mjs (`mise run web:bench`).
 */
import { browserRuntimeDeps } from './browser-deps';
import { browserRuntime, type CoupledStats } from '@digi/workbench-core/runtime/runtime.ts';
import { fetchLocalFirmware, fetchLocalInputs } from '@digi/workbench-core/runtime/local-inputs.ts';

declare global {
  interface Window {
    __bench?: Record<string, unknown>;
  }
}

const params = () => new URLSearchParams(location.search);
export const benchRequested = () => params().get('bench') !== null;

interface Sample {
  wall_s: number;
  audio_s: number;
  dsp_instructions: number;
  cf_instructions: number;
  busy_s: number;
  dsp_busy_s: number;
  cf_wait_s: number;
}

export async function runBench() {
  const query = params();
  const seconds = Number(query.get('seconds') ?? 10);
  const trigAfter = Number(query.get('trig_after') ?? 0);
  const publish = (value: Record<string, unknown>) => {
    window.__bench = value;
    if (value.state !== 'running') console.log(`BENCH_JSON ${JSON.stringify(value)}`);
  };
  publish({ state: 'running' });
  try {
    const [inputs, syx] = await Promise.all([fetchLocalInputs(), fetchLocalFirmware()]);
    if (!inputs) throw new Error('local DSP inputs are missing (dev server only)');
    if (!syx) throw new Error('local firmware is missing (set DIGI_EMU_SYX)');
    const runtime = browserRuntime(() => {}, browserRuntimeDeps());
    if (!runtime.loadCoupled || !(await runtime.coupledAvailable?.()))
      throw new Error('no coupled core (apps/web/.local/emulator-core-sharc.wasm)');
    const channel = new MessageChannel();
    let pcmMessages = 0;
    let pcmPeak = 0;
    channel.port2.onmessage = ({ data }: MessageEvent<Float32Array>) => {
      pcmMessages += 1;
      for (let i = 0; i < data.length; i += 1) {
        const v = Math.abs(data[i]);
        if (v > pcmPeak) pcmPeak = v;
      }
    };
    const loadStart = performance.now();
    await runtime.loadCoupled(
      { syx, image: inputs.image, dsp: inputs.state, snapshot: inputs.snapshot },
      channel.port1,
    );
    const start = performance.now();
    const loadMs = start - loadStart;
    let trigged = false;
    const samples: Sample[] = [];
    let last: CoupledStats | undefined;
    for (;;) {
      const stats = await runtime.coupledStats!();
      last = stats;
      const audio = stats.pcm_produced_seconds;
      if (!trigged && audio >= trigAfter) {
        await runtime.tap!(25);
        trigged = true;
      }
      samples.push({
        wall_s: (performance.now() - start) / 1000,
        audio_s: audio,
        dsp_instructions: stats.dsp_instructions,
        cf_instructions: stats.cf_icount ?? 0,
        busy_s: Number(stats.host.step_response_wall_ms ?? 0) / 1000,
        dsp_busy_s: (stats.split?.dsp_worker_busy_ms ?? 0) / 1000,
        cf_wait_s: (stats.split?.cf_link_wait_ms ?? 0) / 1000,
      });
      if (stats.halted) throw new Error(`DSP halted: ${stats.halted}`);
      if (audio >= seconds) break;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    const first = samples[0],
      end = samples[samples.length - 1];
    // Rate per whole audio second: shows V8 tier-up (slow first seconds, faster later).
    const windows: { audio_s: number; wall_s: number; rate: number }[] = [];
    let from = first;
    for (const sample of samples) {
      if (sample.audio_s - from.audio_s >= 1) {
        windows.push({
          audio_s: +(sample.audio_s - from.audio_s).toFixed(3),
          wall_s: +(sample.wall_s - from.wall_s).toFixed(3),
          rate: +((sample.audio_s - from.audio_s) / (sample.wall_s - from.wall_s)).toFixed(4),
        });
        from = sample;
      }
    }
    const audio = end.audio_s - first.audio_s,
      wall = end.wall_s - first.wall_s;
    const half = Math.floor(windows.length / 2);
    const mean = (list: typeof windows) =>
      list.length
        ? +(list.reduce((a, w) => a + w.audio_s, 0) / list.reduce((a, w) => a + w.wall_s, 0)).toFixed(4)
        : null;
    if (query.get('hold') === null) runtime.dispose();
    publish({
      state: 'done',
      user_agent: navigator.userAgent,
      seconds_target: seconds,
      load_s: +(loadMs / 1000).toFixed(3),
      audio_s: +audio.toFixed(3),
      wall_s: +wall.toFixed(3),
      audio_per_wall: +(audio / wall).toFixed(4),
      rate_first_half: mean(windows.slice(0, half)),
      rate_last_half: mean(windows.slice(half)),
      mode: last?.split ? 'split' : 'single',
      cross_origin_isolated: (globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated === true,
      // Per worker: one worker's step time; split mode reports the ColdFire worker without its time blocked on the DSP, and the DSP worker's time answering and rendering.
      ...(last?.split
        ? {
            cf_worker_busy_s: +(end.busy_s - first.busy_s - (end.cf_wait_s - first.cf_wait_s)).toFixed(3),
            cf_worker_busy_per_audio_s: +(
              (end.busy_s - first.busy_s - (end.cf_wait_s - first.cf_wait_s)) /
              audio
            ).toFixed(4),
            cf_worker_blocked_per_audio_s: +((end.cf_wait_s - first.cf_wait_s) / audio).toFixed(4),
            dsp_worker_busy_s: +(end.dsp_busy_s - first.dsp_busy_s).toFixed(3),
            dsp_worker_busy_per_audio_s: +((end.dsp_busy_s - first.dsp_busy_s) / audio).toFixed(4),
          }
        : {}),
      worker_busy_s: +(end.busy_s - first.busy_s).toFixed(3),
      worker_busy_fraction: +((end.busy_s - first.busy_s) / wall).toFixed(4),
      worker_cpu_per_audio_s: +((end.busy_s - first.busy_s) / audio).toFixed(4),
      dsp_instructions: end.dsp_instructions - first.dsp_instructions,
      cf_instructions: end.cf_instructions - first.cf_instructions,
      dsp_instr_per_audio_s: Math.round((end.dsp_instructions - first.dsp_instructions) / audio),
      cf_instr_per_audio_s: Math.round((end.cf_instructions - first.cf_instructions) / audio),
      fast_param: query.get('fast'),
      dsp_kernels: last?.dsp_kernels ?? null,
      dsp_fast: last?.dsp_fast ?? null,
      pcm_messages: pcmMessages,
      pcm_peak: +pcmPeak.toFixed(4),
      missing_blocks: last?.missing_blocks ?? null,
      trig_pressed: trigged,
      dsp_halted: last?.halted ?? null,
      windows,
    });
  } catch (error) {
    publish({ state: 'error', error: String(error) });
  }
}
