import { createEffect, on, onCleanup, onMount } from 'solid-js';
import { Application, Graphics } from 'pixi.js';
import 'pixi.js/unsafe-eval';
import { SPECTRUM_FLOOR, type AnalyzerFrame } from './analyzer-frame';

export type AnalyzerMode = 'Scope' | 'Spectrum';

export default function AnalyzerPlot(props: {
  frame: AnalyzerFrame | null;
  mode: AnalyzerMode;
  cursor: number | null;
  showHold: boolean;
  unavailable: (message: string) => void;
}) {
  let surface!: HTMLDivElement;
  let paint: (() => void) | undefined;
  let disposed = false;
  let release: (() => void) | undefined;

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
      if (!disposed) props.unavailable(error instanceof Error ? error.message : String(error));
      return;
    }
    if (disposed) {
      app.destroy({ removeView: true }, { children: true });
      return;
    }
    const graphic = new Graphics();
    const colors = getComputedStyle(surface);
    const amber = colors.getPropertyValue('--amber').trim() || '#f3b52c';
    const teal = colors.getPropertyValue('--teal').trim() || '#6fc7d1';
    const grid = colors.getPropertyValue('--ink-3').trim() || '#868b94';
    app.stage.addChild(graphic);
    surface.append(app.canvas);

    function draw() {
      const { width, height } = app.screen;
      const frame = props.frame;
      const scope = props.mode === 'Scope';
      graphic.clear();
      for (let column = 1; column < 8; column++) {
        const x = (column * width) / 8;
        graphic.moveTo(x, 0).lineTo(x, height);
      }
      for (let row = 1; row < 4; row++) {
        const y = (row * height) / 4;
        graphic.moveTo(0, y).lineTo(width, y);
      }
      graphic.stroke({ color: grid, width: 1, alpha: 0.2 });
      if (scope)
        graphic
          .moveTo(0, height / 2)
          .lineTo(width, height / 2)
          .stroke({ color: grid, width: 1, alpha: 0.4 });

      function trace(samples: Float32Array, color: string, alpha: number) {
        const points = Math.min(1024, samples.length);
        if (points < 2) return;
        for (let point = 0; point < points; point++) {
          const fraction = point / (points - 1);
          const sample = samples[Math.round(fraction * (samples.length - 1))];
          const normalized = scope ? (sample + 1) / 2 : 1 - sample / SPECTRUM_FLOOR;
          const y = 1 + (1 - Math.max(0, Math.min(1, normalized))) * (height - 2);
          if (point === 0) graphic.moveTo(0, y);
          else graphic.lineTo(fraction * width, y);
        }
        graphic.stroke({ color, width: 1.25, alpha });
      }

      if (!scope && props.showHold && frame?.heldSpectrum) {
        trace(frame.heldSpectrum[0], amber, 0.35);
        trace(frame.heldSpectrum[1], teal, 0.35);
      }
      const traces = scope ? frame?.scope : frame?.spectrum;
      if (traces) {
        trace(traces[0], amber, 0.95);
        trace(traces[1], teal, 0.9);
      }
      if (props.cursor !== null) {
        const x = props.cursor * width;
        graphic.moveTo(x, 0).lineTo(x, height).stroke({ color: grid, width: 1, alpha: 0.95 });
      }
      app.render();
    }

    let animation = 0;
    paint = () => {
      if (animation) return;
      animation = requestAnimationFrame(() => {
        animation = 0;
        draw();
      });
    };
    const resize = new ResizeObserver(() => {
      app.renderer.resize(Math.max(1, surface.clientWidth), Math.max(1, surface.clientHeight));
      paint?.();
    });
    resize.observe(surface);
    release = () => {
      if (animation) cancelAnimationFrame(animation);
      resize.disconnect();
      app.destroy({ removeView: true }, { children: true });
    };
    paint();
  });

  createEffect(
    on(
      () => [props.frame, props.mode, props.cursor, props.showHold],
      () => paint?.(),
    ),
  );

  onCleanup(() => {
    disposed = true;
    release?.();
    paint = undefined;
  });

  return <div ref={surface} class="analyzer-canvas" aria-hidden="true" />;
}
