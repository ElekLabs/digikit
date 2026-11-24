import { Application, Graphics } from 'pixi.js';
import 'pixi.js/unsafe-eval';
import type { TraceCapture } from '@digi/workbench-core/profile/trace.ts';
import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from 'solid-js';
import {
  focusTraceEvent,
  formatTraceTicks,
  minimumTraceSpan,
  moveTraceView,
  traceBars,
  traceHit,
  traceRuler,
  traceTimeAt,
  traceTimeline,
  zoomTraceView,
  type TraceView,
} from './trace-timeline';
import './trace-timeline.css';

const LANE_HEIGHT = 28;
const MAX_VISIBLE_LANES = 9;
const BAR_INSET = 5;
const LANE_COLORS = ['--amber', '--teal', '--ok', '--ink-2'];
const laneColor = (index: number) => LANE_COLORS[index % LANE_COLORS.length];

type TraceModel = ReturnType<typeof traceTimeline>;

export default function TraceTimeline(props: { trace?: TraceCapture; names: ReadonlyMap<number, string> }) {
  const model = createMemo(() => props.trace && traceTimeline(props.trace));
  const hasClock = () => {
    const hz = props.trace?.hz;
    return hz !== undefined && Number.isFinite(hz) && hz > 0;
  };
  const plottable = () => {
    const timeline = model();
    return hasClock() && timeline && (timeline.lanes.length || timeline.markers.length) ? timeline : undefined;
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

function TraceCounts(props: { trace: TraceCapture; lanes?: number }) {
  const events = () => props.trace.events.length;
  return (
    <span class="trace-timeline-counts">
      {events().toLocaleString()} {events() === 1 ? 'event' : 'events'}
      <Show when={props.lanes !== undefined}>
        {' '}
        · {props.lanes} {props.lanes === 1 ? 'lane' : 'lanes'}
      </Show>
      {' · '}
      <span
        classList={{ 'trace-timeline-dropped': props.trace.dropped > 0 }}
        title={
          props.trace.dropped > 0
            ? 'Some calls are absent from this trace. Empty regions do not establish idle time.'
            : undefined
        }
      >
        {props.trace.dropped.toLocaleString()} dropped{props.trace.dropped > 0 ? ' · partial trace' : ''}
      </span>
    </span>
  );
}

function TimelinePlot(props: { model: TraceModel; trace: TraceCapture; names: ReadonlyMap<number, string> }) {
  let scroller!: HTMLDivElement;
  let plot!: HTMLDivElement;
  let surface!: HTMLDivElement;
  let resize: ResizeObserver | undefined;
  let paint: (() => void) | undefined;
  let release: (() => void) | undefined;
  let disposed = false;
  let drag: { pointer: number; x: number; start: number; moved: boolean } | undefined;

  const [view, setView] = createSignal<TraceView>({ start: 0, span: 1 });
  const [size, setSize] = createSignal({ width: 0, height: 0 });
  const [scrollTop, setScrollTop] = createSignal(0);
  const [cursor, setCursor] = createSignal<{ x: number; y: number }>();
  const [dragging, setDragging] = createSignal(false);
  const [selected, setSelected] = createSignal<{
    event: TraceModel['lanes'][number]['events'][number];
    owner: number;
  }>();
  const [renderError, setRenderError] = createSignal('');
  const height = () => Math.max(1, Math.min(MAX_VISIBLE_LANES, props.model.lanes.length)) * LANE_HEIGHT;
  const atFit = () => view().span === props.model.end;
  const scrollsVertically = () => props.model.lanes.length > MAX_VISIBLE_LANES;
  const ruler = createMemo(() => traceRuler(view(), props.model.origin, props.trace.hz, size().width));
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
      lane && yInLane >= BAR_INSET && yInLane < LANE_HEIGHT - BAR_INSET && traceHit(lane, view(), width, point.x, 3);
    return { ...point, ticks: traceTimeAt(view(), point.x, width), event: event || undefined, owner: lane?.owner };
  });

  const inspected = createMemo(() => {
    const point = hover();
    return point?.event && point.owner !== undefined ? { event: point.event, owner: point.owner } : selected();
  });

  function fit() {
    setSelected(undefined);
    setView({ start: 0, span: props.model.end });
  }

  createEffect(
    on(
      () => props.model,
      () => {
        fit();
        setCursor(undefined);
        drag = undefined;
        setDragging(false);
        scroller.scrollTop = 0;
        setScrollTop(0);
      },
    ),
  );

  const canFitEvents = () => {
    const range = props.model.retained;
    return range && (range.start > 0 || range.end < props.model.end);
  };

  function fitEvents() {
    const range = props.model.retained;
    if (!range) return;
    setSelected(undefined);
    const end = props.model.end;
    const span = Math.min(end, Math.max(minimumTraceSpan(end), range.end - range.start));
    setView(moveTraceView({ start: 0, span }, end, range.start));
  }

  function zoom(factor: number, anchor = 0.5) {
    setView((previous) => zoomTraceView(previous, props.model.end, anchor, factor));
  }

  function pan(fraction: number) {
    setView((previous) => moveTraceView(previous, props.model.end, previous.start + previous.span * fraction));
  }

  function inspect(event: MouseEvent) {
    const bounds = plot.getBoundingClientRect();
    setCursor({ x: event.clientX - bounds.left, y: event.clientY - bounds.top });
  }

  function startPan(event: PointerEvent) {
    if (event.button !== 0 || event.pointerType === 'touch' || atFit()) return;
    drag = { pointer: event.pointerId, x: event.clientX, start: view().start, moved: false };
    plot.setPointerCapture(event.pointerId);
  }

  function movePointer(event: PointerEvent) {
    if (drag?.pointer === event.pointerId) {
      const dx = event.clientX - drag.x;
      if (drag.moved || Math.abs(dx) >= 4) {
        drag.moved = true;
        setDragging(true);
        setCursor(undefined);
        const start = drag.start - (dx / Math.max(1, size().width)) * view().span;
        setView(moveTraceView(view(), props.model.end, start));
        return;
      }
    }
    inspect(event);
  }

  function endPan(event: PointerEvent) {
    if (drag?.pointer !== event.pointerId) return;
    drag = undefined;
    setDragging(false);
  }

  function inspectCloser(event: MouseEvent) {
    inspect(event);
    const point = hover();
    if (point?.event && point.owner !== undefined) {
      setSelected({ event: point.event, owner: point.owner });
      setCursor(undefined);
      setView(focusTraceEvent(point.event, props.model.end));
    } else if (point) {
      setSelected(undefined);
      zoom(0.1, point.x / size().width);
    }
  }

  function wheel(event: WheelEvent) {
    const bounds = plot.getBoundingClientRect();
    const scale = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? bounds.height : 1;
    const horizontal = event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY);
    if (!event.ctrlKey && !event.metaKey && horizontal) {
      if (!atFit()) {
        event.preventDefault();
        pan(((event.deltaX || event.deltaY) * scale) / Math.max(1, bounds.width));
      }
    } else if (event.deltaY && (!scrollsVertically() || event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      inspect(event);
      const anchor = Math.max(0, Math.min(1, (event.clientX - bounds.left) / Math.max(1, bounds.width)));
      zoom(Math.exp(Math.max(-1, Math.min(1, event.deltaY * scale * 0.005))), anchor);
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
      case 'Escape':
        setSelected(undefined);
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
      for (const tick of ruler().ticks) {
        const x = Math.round(tick.fraction * width) + 0.5;
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
      for (const marker of props.model.markers) {
        if (marker.offset < range.start || marker.offset > range.start + range.span) continue;
        const x = Math.min(width - 0.5, Math.floor(((marker.offset - range.start) / range.span) * width) + 0.5);
        graphic.moveTo(x, 0).lineTo(x, height);
      }
      graphic.stroke({ color: styles.getPropertyValue('--teal').trim() || '#6fc7d1', width: 1, alpha: 0.9 });
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
        <div class="trace-timeline-controls">
          <TraceCounts trace={props.trace} lanes={props.model.lanes.length} />
          <Show
            when={
              props.trace.dropped > 0 &&
              props.model.retained &&
              props.model.retained.end < props.model.end &&
              props.model.retained
            }
          >
            {(range) => (
              <span class="trace-timeline-counts">
                · Retained through {elapsed(range().end, props.trace.hz / 1000)}
              </span>
            )}
          </Show>
        </div>
        <div class="trace-timeline-controls" role="group" aria-label="Trace zoom">
          <span class="trace-timeline-window" title="Duration of the visible time window">
            {time(view().span, view().span / 1000)} visible
          </span>
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
          <Show when={canFitEvents()}>
            <button type="button" onClick={fitEvents}>
              Fit events
            </button>
          </Show>
        </div>
      </header>
      <Show when={props.model.markers.length}>
        <div class="trace-timeline-controls" role="group" aria-label="Workload markers">
          <For each={props.model.markers}>
            {(marker) => (
              <button
                type="button"
                title={time(marker.tick)}
                onClick={() => {
                  setSelected(undefined);
                  const span = Math.min(view().span, Math.max(minimumTraceSpan(props.model.end), props.model.end / 4));
                  setView(moveTraceView({ start: 0, span }, props.model.end, marker.offset - span / 2));
                }}
              >
                {marker.name} · {time(marker.tick, props.trace.hz / 1000)}
              </button>
            )}
          </For>
        </div>
      </Show>
      <div class="trace-timeline-inspector">
        <span>
          {scrollsVertically() ? 'Ctrl/⌘ + scroll' : 'Scroll'} to zoom · Drag to pan · Double-click a call to inspect
        </span>
        <Show when={!renderError() && hover()}>
          {(point) => (
            <span class="trace-timeline-time">
              Cursor {elapsed(point().ticks, view().span / Math.max(1, size().width))}
            </span>
          )}
        </Show>
        <Show when={!renderError() && inspected()}>
          {(call) => (
            <>
              <strong>{ownerName(call().owner)}</strong>
              <span>Start {time(call().event.startTicks)}</span>
              <span>Duration {time(call().event.duration)}</span>
            </>
          )}
        </Show>
      </div>
      <div class="trace-timeline-frame" style={{ '--trace-lane-height': `${LANE_HEIGHT}px` }}>
        <div class="trace-timeline-ruler" aria-hidden="true">
          <span class="trace-timeline-ruler-label">ColdFire owner</span>
          <div class="trace-timeline-ruler-scale" style={{ width: `${size().width}px` }}>
            <For each={ruler().ticks}>
              {(tick) => (
                <span
                  style={{
                    left: `${tick.fraction * 100}%`,
                    transform: `translateX(clamp(${-tick.fraction * size().width}px, -50%, calc(${(1 - tick.fraction) * size().width}px - 100%)))`,
                  }}
                >
                  {time(tick.at, ruler().step / 10)}
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
              aria-label="Trace plot. Use plus and minus to zoom and arrow keys to pan."
              classList={{ 'trace-timeline-dragging': dragging() }}
              tabIndex={0}
              onKeyDown={navigate}
              onDblClick={inspectCloser}
              onPointerDown={startPan}
              onPointerMove={movePointer}
              onPointerLeave={() => setCursor(undefined)}
              onPointerUp={endPan}
              onPointerCancel={(event) => {
                endPan(event);
                setCursor(undefined);
              }}
              onLostPointerCapture={endPan}
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
    </>
  );
}
