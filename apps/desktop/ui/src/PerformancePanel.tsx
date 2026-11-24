import DspBlocks from './DspBlocks';
import TraceTimeline from './TraceTimeline';
import CaptureToolbar from './CaptureToolbar';
import type { PerformanceCapture } from '@digi/workbench-core/profile/performance-capture.ts';
import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from 'solid-js';
import { Application, Graphics } from 'pixi.js';
import 'pixi.js/unsafe-eval';
import { percent } from './device-health';
import {
  UTILIZATION_WINDOW_MS as WINDOW_MS,
  utilizationAt,
  utilizationIsFresh,
  type UtilizationPoint,
} from './utilization-history';
import SpanBreakdown from './SpanBreakdown';
import type { SpanRecording } from './span-recording';
import type { createSavedPerformance } from './saved-performance';
import './performance-panel.css';

const hoverPercent = (value: number | null) => (value === null ? '—' : `${value.toFixed(1)}%`);

function hasValue(value: number | null): value is number {
  return value !== null && Number.isFinite(value);
}

const views = ['Utilization', 'Spans', 'Timeline', 'DSP blocks'] as const;
export type PerformanceView = (typeof views)[number];

interface PerformanceProps {
  points: readonly UtilizationPoint[];
  now: number;
  estimated: boolean;
  view: PerformanceView;
  setView(view: PerformanceView): void;
  showProfile(): void;
  recording?: SpanRecording;
  activeTarget: boolean;
  savedPerformance: ReturnType<typeof createSavedPerformance>;
}

export default function PerformancePanel(props: PerformanceProps) {
  const captureView = () => props.view === 'Timeline' || props.view === 'DSP blocks';
  const saved = () => props.savedPerformance.current();
  const trace = () => (saved() ? saved()?.trace : props.activeTarget ? props.recording?.frozenTrace() : undefined);
  const dsp = () => (saved() ? saved()?.dsp : props.activeTarget ? props.recording?.dspBlocks() : undefined);
  const liveStatus = (status?: string) =>
    props.activeTarget ? (status ?? 'Reading target capabilities…') : 'Target unavailable';
  const selectSaved = (capture: PerformanceCapture) => {
    if (capture.hasHotspots || (!capture.dsp && !capture.trace)) props.showProfile();
    if ((props.view === 'DSP blocks' && capture.dsp) || (props.view === 'Timeline' && capture.trace)) return;
    if (capture.dsp) props.setView('DSP blocks');
    else if (capture.trace) props.setView('Timeline');
  };
  const latest = () => {
    const point = props.points.findLast((p) => p.at <= props.now);
    return point && utilizationIsFresh(props.now - point.at, props.estimated) ? point : undefined;
  };
  function latestPercent(series: 'cf' | 'dsp') {
    const value = latest()?.[series] ?? null;
    return percent(hasValue(value) ? value : null);
  }

  return (
    <section
      class="performance-panel"
      classList={{
        'performance-panel-timeline': props.view === 'Timeline',
        'performance-panel-dsp': props.view === 'DSP blocks',
      }}
      aria-label="Performance"
    >
      <header class="performance-header">
        <div class="performance-modes" role="group" aria-label="Performance view">
          <For each={views}>
            {(view) => (
              <button type="button" aria-pressed={props.view === view} onClick={() => props.setView(view)}>
                {view}
              </button>
            )}
          </For>
        </div>
        <Show when={props.view === 'Spans' || props.view === 'Utilization'}>
          <dl class="performance-legend" aria-label="Latest utilization">
            <div class="performance-cf">
              <dt>CF</dt>
              <dd>{latestPercent('cf')}</dd>
            </div>
            <div class="performance-dsp">
              <dt>DSP</dt>
              <dd>{latestPercent('dsp')}</dd>
            </div>
          </dl>
          <span class="performance-source">{props.estimated ? 'Estimated utilization' : 'Measured utilization'}</span>
        </Show>
      </header>
      <CaptureToolbar
        recording={props.recording}
        activeTarget={props.activeTarget}
        saved={props.savedPerformance}
        onSelect={selectSaved}
        showProfile={props.showProfile}
      />
      <Show when={captureView()} fallback={<UtilizationChart {...props} />}>
        <Show
          when={props.view === 'Timeline'}
          fallback={
            <Show
              when={dsp()}
              fallback={
                <p role="status">
                  {saved() ? 'This capture has no DSP block counters.' : liveStatus(props.recording?.dspBlockStatus())}
                </p>
              }
            >
              {(capture) => <DspBlocks capture={capture()} />}
            </Show>
          }
        >
          <Show
            when={trace()}
            fallback={
              <p role="status">
                {saved() ? 'This capture has no ColdFire trace.' : liveStatus(props.recording?.timelineStatus())}
              </p>
            }
          >
            {(capture) => <TraceTimeline trace={capture().trace} names={capture().names} />}
          </Show>
        </Show>
      </Show>
    </section>
  );
}

function UtilizationChart(props: PerformanceProps) {
  let surface!: HTMLDivElement;
  let paint: (() => void) | undefined;
  let release: (() => void) | undefined;
  let disposed = false;
  const [renderError, setRenderError] = createSignal('');

  const windowPoints = createMemo(() =>
    props.points.filter(({ at }) => at >= props.now - WINDOW_MS && at <= props.now),
  );
  const [cursor, setCursor] = createSignal<number>();
  const showingSpans = () => props.view === 'Spans';
  const intervals = createMemo(() =>
    showingSpans()
      ? (props.recording?.intervals().filter((i) => i.end > props.now - WINDOW_MS && i.start <= props.now) ?? [])
      : [],
  );
  const [held, setHeld] = createSignal<number>();
  const hoveredInterval = createMemo(() => {
    const x = cursor();
    if (x === undefined || !showingSpans()) return;
    const at = props.now - (1 - x) * WINDOW_MS;
    return intervals().findLast((i) => i.start <= at && at <= i.end);
  });
  const heldInterval = () => intervals().find((i) => i.end === held());
  const selectedInterval = () => (cursor() !== undefined ? hoveredInterval() : (heldInterval() ?? intervals().at(-1)));
  function leavePlot() {
    if (showingSpans() && cursor() !== undefined) setHeld(hoveredInterval()?.end);
    setCursor(undefined);
  }
  createEffect(
    on([() => props.recording, showingSpans], () => {
      setHeld(undefined);
      setCursor(undefined);
    }),
  );
  const inspected = createMemo(() => {
    const x = cursor();
    if (x === undefined || showingSpans()) return;
    return utilizationAt(windowPoints(), Math.round(props.now - (1 - x) * WINDOW_MS), props.estimated);
  });
  const cursorX = () => cursor() ?? 0;
  function inspect(event: PointerEvent) {
    const bounds = surface.getBoundingClientRect();
    setCursor(Math.max(0, Math.min(1, (event.clientX - bounds.left - 3) / Math.max(1, bounds.width - 6))));
  }
  const peak = createMemo(() =>
    windowPoints().reduce((max, { cf, dsp }) => Math.max(max, hasValue(cf) ? cf : 0, hasValue(dsp) ? dsp : 0), 0),
  );
  const ceiling = () => (peak() <= 100 ? 100 : (Math.floor(peak() / 25) + 1) * 25);
  const hasSamples = () => windowPoints().some(({ cf, dsp }) => hasValue(cf) || hasValue(dsp));

  onMount(async () => {
    const app = new Application();
    try {
      await app.init({
        preference: 'webgl',
        backgroundAlpha: 0,
        antialias: true,
        autoStart: false,
        autoDensity: true,
        resolution: Math.min(2, window.devicePixelRatio),
      });
    } catch (error) {
      if (!disposed) setRenderError(error instanceof Error ? error.message : String(error));
      return;
    }
    if (disposed) {
      app.destroy({ removeView: true }, { children: true });
      return;
    }

    const graphic = new Graphics();
    const canvas = app.canvas;
    app.stage.addChild(graphic);
    surface.append(canvas);

    function draw() {
      const { width, height } = app.screen;
      const styles = getComputedStyle(surface);
      const amber = styles.getPropertyValue('--amber').trim() || '#f3b52c';
      const teal = styles.getPropertyValue('--teal').trim() || '#6fc7d1';
      const grid = styles.getPropertyValue('--ink-3').trim() || '#868b94';
      const start = props.now - WINDOW_MS;
      const top = ceiling();
      const inset = 3;
      const chartWidth = Math.max(0, width - inset * 2);
      const chartHeight = Math.max(0, height - inset * 2);
      const xAt = (fraction: number) => inset + fraction * chartWidth;
      const yAt = (fraction: number) => inset + (1 - fraction) * chartHeight;

      graphic.clear();
      for (const fraction of [0.25, 0.5, 0.75]) {
        const x = xAt(fraction);
        graphic.moveTo(x, 0).lineTo(x, height);
      }
      const mid = yAt(0.5);
      graphic.moveTo(0, mid).lineTo(width, mid).stroke({ color: grid, width: 1, alpha: 0.25 });
      if (top > 100) {
        const y = yAt(100 / top);
        graphic.moveTo(0, y).lineTo(width, y).stroke({ color: grid, width: 1, alpha: 0.45 });
      }

      function trace(series: 'cf' | 'dsp', color: string) {
        let previous: { at: number; x: number; y: number } | undefined;
        let connected = false;
        function finish() {
          if (!previous) return;
          if (connected) graphic.stroke({ color, width: 1.5 });
          else graphic.circle(previous.x, previous.y, 2).fill(color);
          previous = undefined;
          connected = false;
        }

        for (const point of windowPoints()) {
          const value = point[series];
          if (!hasValue(value)) {
            finish();
            continue;
          }
          if (previous && !utilizationIsFresh(point.at - previous.at, props.estimated)) finish();
          const x = xAt((point.at - start) / WINDOW_MS);
          const y = yAt(Math.max(0, value) / top);
          if (previous) {
            if (!connected) graphic.moveTo(previous.x, previous.y);
            graphic.lineTo(x, y);
            connected = true;
          }
          previous = { at: point.at, x, y };
        }
        finish();
      }

      for (const interval of intervals()) {
        const left = xAt(Math.max(0, (interval.start - start) / WINDOW_MS));
        const right = xAt(Math.min(1, (interval.end - start) / WINDOW_MS));
        const width = Math.max(1, right - left - 1);
        if (interval === selectedInterval()) graphic.rect(left, 0, width, height).fill({ color: amber, alpha: 0.12 });
        graphic.rect(left, height - 4, width, 4).fill({ color: amber, alpha: 0.65 });
      }
      trace('cf', amber);
      trace('dsp', teal);
      app.render();
    }

    let animation = 0;
    let contextLost = false;
    paint = () => {
      if (animation || contextLost) return;
      animation = requestAnimationFrame(() => {
        animation = 0;
        try {
          draw();
          setRenderError('');
        } catch (error) {
          setRenderError(error instanceof Error ? error.message : String(error));
        }
      });
    };
    const lost = (event: Event) => {
      event.preventDefault();
      contextLost = true;
      if (animation) cancelAnimationFrame(animation);
      animation = 0;
      setRenderError('Graphics context lost');
    };
    const restored = () => {
      contextLost = false;
      paint?.();
    };
    canvas.addEventListener('webglcontextlost', lost);
    canvas.addEventListener('webglcontextrestored', restored);
    const resize = new ResizeObserver(() => {
      try {
        app.renderer.resize(Math.max(1, surface.clientWidth), Math.max(1, surface.clientHeight));
        paint?.();
      } catch (error) {
        setRenderError(error instanceof Error ? error.message : String(error));
      }
    });
    resize.observe(surface);
    release = () => {
      if (animation) cancelAnimationFrame(animation);
      resize.disconnect();
      canvas.removeEventListener('webglcontextlost', lost);
      canvas.removeEventListener('webglcontextrestored', restored);
      app.destroy({ removeView: true }, { children: true });
    };
    paint();
  });

  createEffect(on([windowPoints, intervals, selectedInterval], () => paint?.()));

  onCleanup(() => {
    disposed = true;
    release?.();
    paint = undefined;
  });

  return (
    <div class="performance-body" classList={{ 'with-spans': showingSpans() }}>
      <div class="performance-chart" role="group" aria-label="Utilization over the last 60 seconds">
        <div class="performance-axis-y">
          <span>{ceiling()}%</span>
          <span>{ceiling() / 2}%</span>
          <span>0%</span>
        </div>
        <div class="performance-plot" onPointerMove={inspect} onPointerLeave={leavePlot} onPointerCancel={leavePlot}>
          <div ref={surface} class="performance-canvas" aria-hidden="true" />
          <Show when={cursor() !== undefined && (hasSamples() || intervals().length > 0) && !renderError()}>
            <div class="performance-overlay">
              <div class="performance-guide" style={{ left: `${cursorX() * 100}%` }}>
                <Show when={!showingSpans() && inspected()}>
                  {(point) => (
                    <>
                      <For each={['cf', 'dsp'] as const}>
                        {(series) => {
                          const value = () => point()[series];
                          return (
                            <Show when={value() != null}>
                              <i
                                class={`performance-marker performance-${series}`}
                                style={{ bottom: `${(100 * value()!) / ceiling()}%` }}
                              />
                            </Show>
                          );
                        }}
                      </For>
                    </>
                  )}
                </Show>
                <div class="performance-tooltip" classList={{ 'align-left': cursorX() > 0.5 }}>
                  <span>{(((1 - cursorX()) * WINDOW_MS) / 1000).toFixed(1)} s ago</span>
                  <Show
                    when={!showingSpans()}
                    fallback={<span>{hoveredInterval() ? 'Recorded interval' : 'No recorded interval'}</span>}
                  >
                    <Show when={inspected()} fallback={<span>No sample</span>}>
                      {(point) => (
                        <>
                          <Show when={point().interpolated}>
                            <span>Interpolated</span>
                          </Show>
                          <span class="performance-cf">CF {hoverPercent(point().cf)}</span>
                          <span class="performance-dsp">DSP {hoverPercent(point().dsp)}</span>
                        </>
                      )}
                    </Show>
                  </Show>
                </div>
              </div>
            </div>
          </Show>
          <Show when={renderError() || (!hasSamples() && !intervals().length)}>
            <div class="performance-plot-status" role="status">
              {renderError()
                ? `Plot unavailable · ${renderError()}`
                : showingSpans()
                  ? 'No recorded intervals'
                  : 'No utilization samples'}
            </div>
          </Show>
        </div>
        <div class="performance-axis-x">
          <span>−60 s</span>
          <span>−30 s</span>
          <span>Now</span>
        </div>
      </div>
      <Show when={showingSpans()}>
        <div class="span-details">
          <label>
            ColdFire interval
            <select
              aria-label="ColdFire interval"
              value={(cursor() !== undefined ? hoveredInterval() : heldInterval())?.end ?? ''}
              onChange={(e) => setHeld(e.currentTarget.value ? Number(e.currentTarget.value) : undefined)}
            >
              <option value="">Latest interval</option>
              <For each={intervals()}>
                {(i) => (
                  <option value={i.end}>
                    {new Date(i.start).toLocaleTimeString()} · {((i.end - i.start) / 1000).toFixed(1)} s
                  </option>
                )}
              </For>
            </select>
          </label>
          <SpanBreakdown interval={selectedInterval()} />
        </div>
      </Show>
    </div>
  );
}
