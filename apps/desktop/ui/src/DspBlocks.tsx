import { Application, Graphics } from 'pixi.js';
import 'pixi.js/unsafe-eval';
import { Index, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from 'solid-js';
import type { DspBlockCapture } from './dsp-block-recording';
import './dsp-blocks.css';

type DspBlockRow = DspBlockCapture['rows'][number];

const formatCount = (value: number) => value.toLocaleString();
const formatMean = (value: number) =>
  value > 0 && value < 0.001 ? '<0.001' : value.toLocaleString(undefined, { maximumFractionDigits: 3 });

export default function DspBlocks(props: { capture: DspBlockCapture }) {
  const ranked = createMemo(() =>
    props.capture.rows
      .filter(({ calls, maxBlock }) => calls > 0 || maxBlock > 0)
      .sort((a, b) => b.meanBlock - a.meanBlock || a.row - b.row),
  );
  const scale = createMemo(() => ranked().reduce((max, row) => Math.max(max, row.maxBlock, row.meanBlock), 0));

  return (
    <section class="dsp-blocks" aria-label="DSP block costs">
      <header class="dsp-blocks-summary">
        <span>
          {formatCount(props.capture.blocks)} {props.capture.blocks === 1 ? 'block' : 'blocks'} captured
        </span>
        <span classList={{ 'dsp-blocks-overruns': props.capture.overruns > 0 }}>
          {formatCount(props.capture.overruns)} {props.capture.overruns === 1 ? 'overrun' : 'overruns'}
        </span>
      </header>
      <Show when={ranked().length} fallback={<p class="dsp-blocks-empty">No active DSP dispatch slots</p>}>
        <div class="dsp-blocks-scroll" role="region" aria-label="Ranked dispatch slots" tabIndex={0}>
          <table
            class="dsp-blocks-table"
            aria-label={`Dispatch costs in ${props.capture.unit}, ranked by mean per block`}
          >
            <thead>
              <tr>
                <th scope="col">Dispatch slot</th>
                <th scope="col">
                  <div class="dsp-blocks-axis">
                    <span>0</span>
                    <span>
                      {scale() > 0 ? `${formatMean(scale())} ` : ''}
                      {props.capture.unit} / block
                    </span>
                  </div>
                </th>
                <th scope="col" class="dsp-blocks-number dsp-blocks-mean">
                  Est. mean / block
                </th>
                <th scope="col" class="dsp-blocks-number dsp-blocks-worst">
                  Worst block
                </th>
                <th scope="col" class="dsp-blocks-number">
                  Calls
                </th>
                <th scope="col" class="dsp-blocks-number">
                  Max call
                </th>
              </tr>
            </thead>
            <tbody>
              <Index each={ranked()}>
                {(row, index) => (
                  <tr>
                    <th scope="row">
                      {row().kind === 'machine' ? 'Machine' : 'Effect'} slot {row().row}
                    </th>
                    <Show when={index === 0}>
                      <td class="dsp-blocks-chart" rowSpan={ranked().length}>
                        <BlockCostPlot rows={ranked()} scale={scale()} />
                      </td>
                    </Show>
                    <td class="dsp-blocks-number dsp-blocks-mean">{formatMean(row().meanBlock)}</td>
                    <td class="dsp-blocks-number dsp-blocks-worst">{formatCount(row().maxBlock)}</td>
                    <td class="dsp-blocks-number">{formatCount(row().calls)}</td>
                    <td class="dsp-blocks-number">{formatCount(row().maxCall)}</td>
                  </tr>
                )}
              </Index>
            </tbody>
          </table>
        </div>
      </Show>
      <Show when={ranked().length}>
        <small>
          Mean cost is estimated. The DSP reports row totals and the block count at different times within a sweep.
        </small>
      </Show>
    </section>
  );
}

function BlockCostPlot(props: { rows: readonly DspBlockRow[]; scale: number }) {
  let surface!: HTMLDivElement;
  let paint: (() => void) | undefined;
  let release: (() => void) | undefined;
  let disposed = false;
  const [renderError, setRenderError] = createSignal(false);

  onMount(async () => {
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
    } catch {
      if (!disposed) setRenderError(true);
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
      const { clientWidth: width, clientHeight: height } = surface;
      if (!width || !height) return;
      if (app.screen.width !== width || app.screen.height !== height) app.renderer.resize(width, height);

      const styles = getComputedStyle(surface);
      const mean = styles.getPropertyValue('--teal').trim() || '#6fc7d1';
      const worst = styles.getPropertyValue('--amber').trim() || '#f3b52c';
      const grid = styles.getPropertyValue('--ink-3').trim() || '#868b94';
      const rows = props.rows;
      const rowHeight = height / rows.length;
      const barHeight = Math.min(6, Math.floor(rowHeight / 4));
      const scale = props.scale;

      graphic.clear();
      for (const fraction of [0.25, 0.5, 0.75]) {
        const x = Math.floor(fraction * width) + 0.5;
        graphic.moveTo(x, 0).lineTo(x, height);
      }
      graphic.stroke({ color: grid, width: 1, alpha: 0.25 });

      if (scale > 0) {
        for (const [index, row] of rows.entries()) {
          const center = Math.round((index + 0.5) * rowHeight);
          if (row.meanBlock > 0)
            graphic.rect(0, center - barHeight - 1, (row.meanBlock / scale) * width, barHeight).fill(mean);
          if (row.maxBlock > 0) graphic.rect(0, center + 1, (row.maxBlock / scale) * width, barHeight).fill(worst);
        }
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
          setRenderError(false);
        } catch {
          setRenderError(true);
        }
      });
    };

    const lost = (event: Event) => {
      event.preventDefault();
      contextLost = true;
      cancelAnimationFrame(animation);
      animation = 0;
      setRenderError(true);
    };
    const restored = () => {
      contextLost = false;
      paint?.();
    };
    canvas.addEventListener('webglcontextlost', lost);
    canvas.addEventListener('webglcontextrestored', restored);
    const resize = new ResizeObserver(() => paint?.());
    resize.observe(surface);

    release = () => {
      resize.disconnect();
      cancelAnimationFrame(animation);
      canvas.removeEventListener('webglcontextlost', lost);
      canvas.removeEventListener('webglcontextrestored', restored);
      app.destroy({ removeView: true }, { children: true });
    };
    paint();
  });

  createEffect(
    on(
      () => props.rows,
      () => paint?.(),
    ),
  );
  onCleanup(() => {
    disposed = true;
    release?.();
    paint = undefined;
  });

  return (
    <>
      <div ref={surface} class="dsp-blocks-canvas" aria-hidden="true" />
      <Show when={renderError()}>
        <div class="dsp-blocks-plot-status" role="status">
          Plot unavailable
        </div>
      </Show>
    </>
  );
}
