import { Application, Graphics } from 'pixi.js';
import 'pixi.js/unsafe-eval';
import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from 'solid-js';
import {
  formatTraceTicks,
  minimumTraceSpan,
  moveTraceView,
  traceBars,
  traceHit,
  traceTimeAt,
  traceTimeline,
  zoomTraceView,
  type TraceTimelineCapture,
  type TraceView,
} from './trace-timeline';
import './trace-timeline.css';

const LANE_HEIGHT = 28;
const MAX_VISIBLE_LANES = 9;
const BAR_INSET = 5;
const LANE_COLORS = ['--amber', '--teal', '--ok', '--ink-2'];
const laneColor = (index: number) => LANE_COLORS[index % LANE_COLORS.length];

type TraceModel = ReturnType<typeof traceTimeline>;

export default function TraceTimeline(props: { trace?: TraceTimelineCapture; names: ReadonlyMap<number, string> }) {
  const model = createMemo(() => props.trace && traceTimeline(props.trace));
  const hasClock = () => {
    const hz = props.trace?.hz;
    return hz !== undefined && Number.isFinite(hz) && hz > 0;
  };
  const plottable = () => {
    const timeline = model();
    return hasClock() && timeline?.lanes.length ? timeline : undefined;
  };

  return (
    <section class="trace-timeline" aria-label="ColdFire trace timeline">
      <Show when={props.trace} fallback={<p class="trace-timeline-empty">No trace captured</p>}>
        {(trace) => (
          <Show
            when={plottable()}
            fallback={
              <>
                <div class="trace-timeline-toolbar">
                  <TraceCounts trace={trace()} />
                </div>
                <p class="trace-timeline-empty">{hasClock() ? 'No trace events' : 'Trace clock unavailable'}</p>
              </>
            }
          >
            {(timeline) => <TimelinePlot model={timeline()} trace={trace()} names={props.names} />}
          </Show>
        )}
      </Show>
    </section>
  );
}

function TraceCounts(props: { trace: TraceTimelineCapture; lanes?: number }) {
  const events = () => props.trace.events.length;
  return (
    <span class="trace-timeline-counts">
      {events().toLocaleString()} {events() === 1 ? 'event' : 'events'}
      <Show when={props.lanes !== undefined}>
        {' '}
        · {props.lanes} {props.lanes === 1 ? 'lane' : 'lanes'}
      </Show>
      {' · '}
      <span classList={{ 'trace-timeline-dropped': props.trace.dropped > 0 }}>
        {props.trace.dropped.toLocaleString()} dropped
      </span>
    </span>
  );
}

function TimelinePlot(props: { model: TraceModel; trace: TraceTimelineCapture; names: ReadonlyMap<number, string> }) {
  let scroller!: HTMLDivElement;
  let plot!: HTMLDivElement;
  let surface!: HTMLDivElement;
  let resize: ResizeObserver | undefined;
  let paint: (() => void) | undefined;
  let release: (() => void) | undefined;
  let disposed = false;

  const [view, setView] = createSignal<TraceView>({ start: 0, span: 1 });
  const [size, setSize] = createSignal({ width: 0, height: 0 });
  const [scrollTop, setScrollTop] = createSignal(0);
  const [cursor, setCursor] = createSignal<{ x: number; y: number }>();
  const [renderError, setRenderError] = createSignal('');
  const height = () => Math.min(MAX_VISIBLE_LANES, props.model.lanes.length) * LANE_HEIGHT;
  const atFit = () => view().span === props.model.end;
  const fractions = createMemo(() => (size().width >= 400 ? [0, 0.25, 0.5, 0.75, 1] : [0, 0.5, 1]));
  const ownerName = (owner: number) => props.names.get(owner) || `0x${owner.toString(16).padStart(8, '0')}`;
  const time = (ticks: number | bigint, resolution?: number) => formatTraceTicks(ticks, props.trace.hz, resolution);
  const elapsed = (offset: number, resolution?: number) =>
    time(props.model.origin + BigInt(Math.round(offset)), resolution);

  const hover = createMemo(() => {
    const point = cursor();
    const { width, height } = size();
    if (!point || point.x < 0 || point.x >= width || point.y < 0 || point.y >= height) return;
    const y = point.y + scrollTop();
    const lane = props.model.lanes[Math.floor(y / LANE_HEIGHT)];
    const yInLane = y % LANE_HEIGHT;
    const event =
      lane && yInLane >= BAR_INSET && yInLane < LANE_HEIGHT - BAR_INSET && traceHit(lane, view(), width, point.x);
    return { ...point, ticks: traceTimeAt(view(), point.x, width), event: event || undefined, owner: lane?.owner };
  });

  function fit() {
    setView({ start: 0, span: props.model.end });
  }

  createEffect(
    on(
      () => props.model,
      () => {
        fit();
        setCursor(undefined);
        scroller.scrollTop = 0;
        setScrollTop(0);
      },
    ),
  );

  function zoom(factor: number, anchor = 0.5) {
    setView((previous) => zoomTraceView(previous, props.model.end, anchor, factor));
  }

  function pan(fraction: number) {
    setView((previous) => moveTraceView(previous, props.model.end, previous.start + previous.span * fraction));
  }

  function inspect(event: PointerEvent) {
    const bounds = plot.getBoundingClientRect();
    setCursor({ x: event.clientX - bounds.left, y: event.clientY - bounds.top });
  }

  function wheel(event: WheelEvent) {
    const bounds = plot.getBoundingClientRect();
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      const anchor = Math.max(0, Math.min(1, (event.clientX - bounds.left) / Math.max(1, bounds.width)));
      const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? bounds.height : 1);
      zoom(Math.exp(Math.max(-1, Math.min(1, delta * 0.005))), anchor);
    } else if (!atFit() && (event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY))) {
      event.preventDefault();
      pan((event.deltaX || event.deltaY) / Math.max(1, bounds.width));
    }
  }

  function navigate(event: KeyboardEvent) {
    switch (event.key) {
      case '+':
      case '=':
        zoom(0.5);
        break;
      case '-':
        zoom(2);
        break;
      case 'ArrowLeft':
        pan(-0.15);
        break;
      case 'ArrowRight':
        pan(0.15);
        break;
      case 'Home':
        fit();
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  onMount(async () => {
    plot.addEventListener('wheel', wheel, { passive: false });
    resize = new ResizeObserver(() => {
      setSize({ width: plot.clientWidth, height: plot.clientHeight });
    });
    resize.observe(plot);

    const app = new Application();
    try {
      await app.init({
        preference: 'webgl',
        backgroundAlpha: 0,
        antialias: false,
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

    const canvas = app.canvas;
    const graphic = new Graphics();
    app.stage.addChild(graphic);
    surface.append(canvas);

    function draw() {
      const { width, height } = size();
      if (!width || !height) return;
      if (app.screen.width !== width || app.screen.height !== height) app.renderer.resize(width, height);
      const styles = getComputedStyle(plot);
      const grid = styles.getPropertyValue('--ink-3').trim() || '#868b94';
      const top = scrollTop();
      const first = Math.floor(top / LANE_HEIGHT);
      const last = Math.min(props.model.lanes.length, Math.ceil((top + height) / LANE_HEIGHT));
      const range = view();

      graphic.clear();
      for (const fraction of fractions().slice(1, -1)) {
        const x = Math.round(fraction * width) + 0.5;
        graphic.moveTo(x, 0).lineTo(x, height);
      }
      for (let i = first + 1; i < last; i++) {
        const y = i * LANE_HEIGHT - top + 0.5;
        graphic.moveTo(0, y).lineTo(width, y);
      }
      graphic.stroke({ color: grid, width: 1, alpha: 0.25 });

      for (let i = first; i < last; i++) {
        const y = i * LANE_HEIGHT - top;
        const color = styles.getPropertyValue(laneColor(i)).trim() || '#f3b52c';
        for (const bar of traceBars(props.model.lanes[i], range, width)) {
          graphic.rect(bar.left, y + BAR_INSET, bar.width, LANE_HEIGHT - BAR_INSET * 2);
        }
        graphic.fill({ color, alpha: 0.74 });
      }
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
      cancelAnimationFrame(animation);
      animation = 0;
      setRenderError('Graphics context lost');
    };
    const restored = () => {
      contextLost = false;
      paint?.();
    };
    canvas.addEventListener('webglcontextlost', lost);
    canvas.addEventListener('webglcontextrestored', restored);
    release = () => {
      cancelAnimationFrame(animation);
      canvas.removeEventListener('webglcontextlost', lost);
      canvas.removeEventListener('webglcontextrestored', restored);
      app.destroy({ removeView: true }, { children: true });
    };
    paint();
  });

  createEffect(on([() => props.model, scrollTop, size, view], () => paint?.()));
  onCleanup(() => {
    disposed = true;
    resize?.disconnect();
    plot?.removeEventListener('wheel', wheel);
    release?.();
    paint = undefined;
  });

  return (
    <>
      <header class="trace-timeline-toolbar">
        <TraceCounts trace={props.trace} lanes={props.model.lanes.length} />
        <div class="trace-timeline-controls" role="group" aria-label="Trace zoom">
          <button type="button" aria-label="Zoom out" disabled={atFit()} onClick={() => zoom(2)}>
            −
          </button>
          <button
            type="button"
            aria-label="Zoom in"
            disabled={view().span <= minimumTraceSpan(props.model.end)}
            onClick={() => zoom(0.5)}
          >
            +
          </button>
          <button type="button" disabled={atFit()} onClick={fit}>
            Fit
          </button>
        </div>
      </header>
      <div class="trace-timeline-frame" style={{ '--trace-lane-height': `${LANE_HEIGHT}px` }}>
        <div class="trace-timeline-ruler" aria-hidden="true">
          <span class="trace-timeline-ruler-label">Owner</span>
          <div class="trace-timeline-ruler-scale" style={{ width: `${size().width}px` }}>
            <For each={fractions()}>
              {(fraction) => (
                <span style={{ left: `${fraction * 100}%` }}>
                  {elapsed(view().start + view().span * fraction, view().span / (fractions().length - 1))}
                </span>
              )}
            </For>
          </div>
        </div>
        <div
          ref={scroller}
          class="trace-timeline-scroll"
          style={{ height: `${height()}px` }}
          onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        >
          <div class="trace-timeline-sticky">
            <div
              ref={plot}
              class="trace-timeline-plot"
              style={{ height: `${height()}px` }}
              role="region"
              aria-label="Trace plot. Control or Command and scroll to zoom. Use arrow keys to pan."
              tabIndex={0}
              onKeyDown={navigate}
              onPointerMove={inspect}
              onPointerLeave={() => setCursor(undefined)}
              onPointerCancel={() => setCursor(undefined)}
            >
              <div ref={surface} class="trace-timeline-canvas" aria-hidden="true" />
              <Show when={!renderError() && hover()}>
                {(point) => (
                  <div
                    class="trace-timeline-crosshair"
                    style={{ '--trace-cursor-x': `${point().x}px`, '--trace-cursor-y': `${point().y}px` }}
                    aria-hidden="true"
                  />
                )}
              </Show>
              <Show when={renderError()}>
                <div class="trace-timeline-empty" role="status">
                  Trace plot unavailable · {renderError()}
                </div>
              </Show>
            </div>
          </div>
          <For each={props.model.lanes}>
            {(lane, index) => (
              <div class="trace-timeline-lane" style={{ '--trace-lane-color': `var(${laneColor(index())})` }}>
                <span title={ownerName(lane.owner)}>{ownerName(lane.owner)}</span>
              </div>
            )}
          </For>
        </div>
      </div>
      <div class="trace-timeline-inspector">
        <Show when={!renderError() && hover()} fallback={<span>Ctrl/⌘ + scroll to zoom · Shift + scroll to pan</span>}>
          {(point) => (
            <>
              <span class="trace-timeline-time">
                Cursor {elapsed(point().ticks, view().span / Math.max(1, size().width))}
              </span>
              <Show when={point().event}>
                {(event) => (
                  <>
                    <strong>{ownerName(point().owner!)}</strong>
                    <span>Start {time(event().startTicks)}</span>
                    <span>Duration {time(event().duration)}</span>
                  </>
                )}
              </Show>
            </>
          )}
        </Show>
      </div>
    </>
  );
}
