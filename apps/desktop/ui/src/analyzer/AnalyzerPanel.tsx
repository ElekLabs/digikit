import { For, Show, Suspense, createEffect, createMemo, createSignal, lazy, on, onCleanup, untrack } from 'solid-js';
import type { AnalyzerHost } from '@digi/workbench-core/host/host.ts';
import type { Target } from '@digi/workbench-core/transfer/target.ts';
import type { WorkerFactory, WorkerPort } from '@digi/workbench-core/workers.ts';
import type { AnalyzerMode } from './AnalyzerPlot';
import {
  FFT_SIZE,
  MIN_PITCH_CYCLES,
  SPECTRUM_FLOOR,
  dbfs,
  frequencyAt,
  type AnalyzerInput,
  type AnalyzerResult,
} from './frame';
import { createAnalyzerStream, type AnalyzerStatus } from './stream';
import './analyzer-panel.css';

const AnalyzerPlot = lazy(() => import('./AnalyzerPlot'));
const CHANNELS = [0, 1] as const;

function formatDb(value: number) {
  return value === -Infinity ? '−∞' : value.toFixed(1);
}

function formatHz(value: number) {
  return value >= 1000 ? `${(value / 1000).toFixed(2)} kHz` : `${value.toFixed(1)} Hz`;
}

function pcmTail(samples: number[], count: number) {
  return Float32Array.from(samples.slice(samples.length - count), (value) => (Number.isFinite(value) ? value : 0));
}

export default function AnalyzerPanel(props: {
  host: AnalyzerHost;
  target: Target;
  workers: WorkerFactory<'analyzer'>;
}) {
  const [mode, setMode] = createSignal<AnalyzerMode>('Scope');
  const [status, setStatus] = createSignal<AnalyzerStatus>(
    { kind: 'pending' },
    {
      equals: (before, after) => before.kind === after.kind && before.kind !== 'error',
    },
  );
  const [trigger, setTrigger] = createSignal(true);
  const [hold, setHold] = createSignal(false);
  const [heldRevision, setHeldRevision] = createSignal(0);
  const [live, setLive] = createSignal<AnalyzerResult | null>(null);
  const [frozen, setFrozen] = createSignal<AnalyzerResult | null>(null);
  const [cursor, setCursor] = createSignal<number | null>(null);
  const [renderError, setRenderError] = createSignal('');
  let revision = 0;
  let generation: number | undefined;
  let rate: number | undefined;
  let busy = false;
  let worker: WorkerPort | undefined;

  const result = () => frozen() ?? live();
  const frame = () => result()?.frame;
  const traces = () => (mode() === 'Scope' ? frame()?.scope : frame()?.spectrum);
  const cursorReadout = createMemo(() => {
    const data = traces();
    const current = frame();
    const at = cursor();
    if (!data || !current || at === null) return '';
    const index = Math.round(at * (data[0].length - 1));
    if (mode() === 'Scope') {
      const ms = (index * 1000) / current.rate;
      return `${ms.toFixed(2)} ms · L ${data[0][index].toFixed(3)} FS · R ${data[1][index].toFixed(3)} FS`;
    }
    const frequency = frequencyAt(index / (data[0].length - 1), current.rate);
    const level = (sample: number) => (sample <= SPECTRUM_FLOOR ? `≤ ${SPECTRUM_FLOOR}` : formatDb(sample));
    return `${formatHz(frequency)} · L ${level(data[0][index])} dBFS · R ${level(data[1][index])} dBFS`;
  });

  function clearDisplay() {
    revision++;
    setLive(null);
    setFrozen(null);
    setCursor(null);
  }

  const stream = createAnalyzerStream(
    untrack(() => props.host),
    (_, pcm) => {
      if (!worker) return;
      if (generation !== pcm.generation || rate !== pcm.rate) {
        generation = pcm.generation;
        rate = pcm.rate;
        clearDisplay();
      }
      const count = Math.min(FFT_SIZE, pcm.left.length, pcm.right.length);
      const left = pcmTail(pcm.left, count);
      const right = pcmTail(pcm.right, count);
      const message: AnalyzerInput = {
        revision,
        rate: pcm.rate,
        left,
        right,
        trigger: trigger(),
        hold: hold(),
        holdRevision: heldRevision(),
      };
      busy = true;
      worker.postMessage(message, [left.buffer, right.buffer]);
    },
    () => !!worker && !busy,
    (update) => {
      if ((update.kind === 'waiting' || update.kind === 'error') && status().kind !== update.kind) clearDisplay();
      setStatus(update);
    },
  );

  function workerFailed(message: string) {
    stream.select(null);
    worker?.terminate();
    worker = undefined;
    busy = false;
    clearDisplay();
    setStatus({ kind: 'error', message });
  }

  createEffect(
    on(
      () => [props.target.kind, props.target.kind === 'midi' ? props.target.port : ''] as const,
      ([kind]) => {
        generation = undefined;
        rate = undefined;
        clearDisplay();
        try {
          if (kind !== 'none' && !worker) {
            worker = props.workers.create('analyzer');
            worker.onmessage = ({ data }: MessageEvent<AnalyzerResult>) => {
              busy = false;
              if (data.revision !== revision) return;
              setLive(data);
            };
            worker.onerror = (error) => workerFailed(error.message);
          }
          stream.select(kind === 'none' ? null : kind === 'midi' ? 'remote' : 'emulator');
        } catch (error) {
          workerFailed(error instanceof Error ? error.message : String(error));
        }
      },
    ),
  );

  onCleanup(() => {
    revision++;
    stream.select(null);
    worker?.terminate();
    worker = undefined;
  });

  const activity = () => {
    const current = status();
    if (props.target.kind === 'none') return 'Select an emulator or USB device to analyze audio.';
    if (current.kind === 'error') return `Audio unavailable · ${current.message}`;
    if (current.kind === 'waiting')
      return props.target.kind === 'midi' ? 'No USB audio samples' : 'No emulator audio samples';
    if (current.kind === 'pending') return 'Waiting for audio samples…';
    if (frozen()) return 'Plot frozen · meters live';
    return props.target.kind === 'midi' ? 'Live USB audio' : 'Live emulator audio';
  };
  const scale = createMemo(() => {
    const current = frame();
    if (mode() === 'Scope') {
      const length = current ? ((current.scope[0].length - 1) * 1000) / current.rate : 10;
      return ['0', `${(length / 2).toFixed(1)} ms`, `${length.toFixed(1)} ms`];
    }
    if (!current) return ['20 Hz', '', ''];
    return [formatHz(20), formatHz(frequencyAt(0.5, current.rate)), formatHz(current.rate / 2)];
  });
  const pitchHint = () => {
    const current = frame();
    return current
      ? `Estimated fundamental · reliable tonal signals above ${formatHz((MIN_PITCH_CYCLES * current.rate) / FFT_SIZE)}`
      : 'Estimated fundamental · requires a strong tonal signal';
  };

  return (
    <section class="analyzer-panel" aria-label="Audio analyzer">
      <div class="analyzer-controls">
        <div class="analyzer-modes" role="group" aria-label="Plot mode">
          <For each={['Scope', 'Spectrum'] as const}>
            {(item) => (
              <button
                type="button"
                aria-pressed={mode() === item}
                onClick={() => {
                  setMode(item);
                  setCursor(null);
                }}
              >
                {item}
              </button>
            )}
          </For>
        </div>
        <Show
          when={mode() === 'Scope'}
          fallback={
            <button
              type="button"
              aria-pressed={hold()}
              onClick={() => {
                setHold(!hold());
                setHeldRevision((before) => before + 1);
              }}
            >
              Peak hold
            </button>
          }
        >
          <button type="button" aria-pressed={trigger()} onClick={() => setTrigger(!trigger())}>
            Rising trigger
          </button>
        </Show>
        <button
          type="button"
          aria-pressed={!!frozen()}
          disabled={!frame()}
          onClick={() => setFrozen(frozen() ? null : live())}
        >
          Freeze
        </button>
        <span class="analyzer-status" role="status">
          {activity()}
        </span>
      </div>
      <div class="analyzer-body">
        <div class="analyzer-chart">
          <div class="analyzer-axis-y" aria-hidden="true">
            <For each={mode() === 'Scope' ? ['+1', '0 FS', '−1'] : ['0', '−50', '−100 dBFS']}>
              {(label) => <span>{label}</span>}
            </For>
          </div>
          <div
            class="analyzer-plot"
            onPointerDown={(event) => {
              if (!traces()) return;
              const bounds = event.currentTarget.getBoundingClientRect();
              setCursor(Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)));
            }}
          >
            <Suspense fallback={<span class="analyzer-plot-status">Loading plot…</span>}>
              <AnalyzerPlot
                frame={frame() ?? null}
                mode={mode()}
                cursor={traces() ? cursor() : null}
                showHold={hold() && result()?.holdRevision === heldRevision()}
                unavailable={setRenderError}
              />
            </Suspense>
            <Show when={renderError()}>
              {(message) => (
                <span class="analyzer-plot-status" role="status" title={message()}>
                  Plot unavailable
                </span>
              )}
            </Show>
          </div>
          <div class="analyzer-axis-x" aria-hidden="true">
            <For each={scale()}>{(label) => <span>{label}</span>}</For>
          </div>
          <div class="analyzer-cursor">
            <label>
              Cursor{' '}
              <input
                aria-label={`${mode()} cursor`}
                type="range"
                min="0"
                max="1000"
                value={(cursor() ?? 0) * 1000}
                disabled={!traces()}
                onInput={(event) => setCursor(event.currentTarget.valueAsNumber / 1000)}
              />
            </label>
            <output>{cursorReadout() || '—'}</output>
            <Show when={mode() === 'Scope' && trigger() && frame()}>
              <span class="analyzer-trigger-status">{frame()?.triggered ? 'Triggered' : 'Free-running'}</span>
            </Show>
          </div>
        </div>
        <table class="analyzer-meters">
          <caption class="analyzer-visually-hidden">Stereo measurements</caption>
          <thead>
            <tr>
              <th />
              <th scope="col">Peak</th>
              <th scope="col">RMS</th>
              <th scope="col" title={pitchHint()}>
                F₀
              </th>
            </tr>
          </thead>
          <tbody>
            <For each={CHANNELS}>
              {(channel) => {
                const meter = () => live()?.frame.meters[channel];
                const pitch = () => frame()?.fundamental[channel];
                return (
                  <tr>
                    <th scope="row" class={channel === 0 ? 'analyzer-left' : 'analyzer-right'}>
                      {channel === 0 ? 'L' : 'R'}
                    </th>
                    <td
                      classList={{ 'analyzer-clipped': !!meter()?.clipped }}
                      title={meter()?.clipped ? 'Clipping' : undefined}
                    >
                      {meter() ? `${formatDb(dbfs(meter()!.peak))} dBFS` : '—'}
                      <Show when={meter()?.clipped}>
                        <span class="analyzer-visually-hidden"> clipping</span>
                      </Show>
                    </td>
                    <td>{meter() ? `${formatDb(dbfs(meter()!.rms))} dBFS` : '—'}</td>
                    <td>{pitch() == null ? '—' : formatHz(pitch()!)}</td>
                  </tr>
                );
              }}
            </For>
          </tbody>
        </table>
      </div>
    </section>
  );
}
