import { createSignal, onCleanup, onMount, type JSX } from 'solid-js';
import type { Panel } from '../data/faceplates';
import { drawFrame, type EmulatorRuntime, type RuntimeSnapshot } from '../runtime/runtime';
import { LED_COUNT, defaultLedColor, ledLight, parseLeds, WARM_WHITE } from '../runtime/leds';
import PanelArt from './PanelArt';

export interface LivePanelApi {
  /** Draw a snapshot's OLED frame and LEDs. */
  show(snapshot: RuntimeSnapshot): void;
  /** Draw an OLED frame alone (1024 bytes, the emulator's frame layout); the Remote page's device screen. */
  showFrame(frame: Uint8Array): void;
  /** Clear the OLED and turn every LED off. */
  clear(): void;
  /** Release every held key and end any knob drag. */
  releaseAll(): void;
  /** Show a fixed LED state (the dev ?leds= preview). */
  previewLeds(leds: Uint8Array): void;
}

export interface LivePanelProps {
  panel: Panel;
  /** Where keys and knobs go: the emulator's runtime, or anything that presses, releases and turns (the Remote page's device). */
  runtime: () => Pick<EmulatorRuntime, 'press' | 'release' | 'turn'> | undefined;
  /** Where key and knob failures go. */
  onError: (message: string) => void;
  /** Called once, on creation. */
  ref?: (api: LivePanelApi) => void;
}

/** The front panel with a live OLED, LEDs, keys and knobs wired to a runtime. */
export default function LivePanel(props: LivePanelProps): JSX.Element {
  const [pressed, setPressed] = createSignal(new Set<number>());
  const [leds, setLeds] = createSignal(new Uint8Array(LED_COUNT));
  let canvas: HTMLCanvasElement | undefined;
  const pointers = new Map<
    number,
    { code?: number; encoder?: number; y: number; pushed?: number; moved: boolean; target?: SVGGElement }
  >();
  const owners = new Map<number, number>();
  // Encoders are endless and the firmware reports no angle: the notch angle is UI-only (24 detents per turn).
  const [angles, setAngles] = createSignal<Record<number, number>>({});
  const angle = (encoder: number | undefined) => (encoder ? (angles()[encoder] ?? 0) : 0);
  const spin = (encoder: number, detents: number) =>
    setAngles((old) => ({ ...old, [encoder]: ((((old[encoder] ?? 0) + detents * 15) % 360) + 360) % 360 }));
  // A mouse cannot hold FUNC while clicking another key, and the firmware only sees a chord when FUNC
  // stays down across the other key's press and release (docs/findings/03-ui-and-panel.md). A click
  // on FUNC latches it; the next key's release (or a second click on FUNC) releases it.
  const FUNC = 17;
  let funcLatched = false;
  let suppressedKnob: SVGGElement | undefined;
  let clearSuppression: number | undefined;
  // A key's lit state: the legend glows (the real keys are backlit through the print). Trig keys default to idx = code.
  const keyLight = (key: { code: number; style?: string; icon?: string; led?: number; led_color?: string }) => {
    const idx = key.led;
    if (idx === undefined) return undefined;
    const light = ledLight(leds()[idx] ?? 0);
    return light.on
      ? { '--lit': String(light.intensity), '--lit-color': light.color ?? key.led_color ?? defaultLedColor(key.icon) }
      : undefined;
  };
  const extraColor = (i: number) =>
    props.panel.leds.extra?.[i - props.panel.leds.x.length * props.panel.leds.y.length]?.color;
  const indicatorLight = (i: number) => {
    const idx = props.panel.leds.idx?.[i];
    if (idx === undefined) return undefined;
    const light = ledLight(leds()[idx] ?? 0);
    return light.on
      ? {
          '--lit': String(light.intensity),
          '--lit-color': light.color ?? extraColor(i) ?? props.panel.leds.color ?? WARM_WHITE,
        }
      : undefined;
  };
  const emit = (code: number, down: boolean) => {
    const count = owners.get(code) ?? 0;
    const nextCount = down ? count + 1 : Math.max(0, count - 1);
    if (nextCount) owners.set(code, nextCount);
    else owners.delete(code);
    if ((down && count === 0) || (!down && count === 1)) {
      setPressed((old) => {
        const next = new Set(old);
        if (down) next.add(code);
        else next.delete(code);
        return next;
      });
      const result = down ? props.runtime()?.press(code) : props.runtime()?.release(code);
      void result?.catch((error) => props.onError(String(error)));
    }
  };
  // Knob drags listen on window, not on the SVG with pointer capture: WebKit (the Tauri webview, Safari)
  // can retarget or drop captured pointer events and fire lostpointercapture early, which ended the drag
  // before the first detent. Deltas come from clientY differences (movementY is not reliable in WebKit).
  const dragMove = (e: PointerEvent) => {
    const drag = pointers.get(e.pointerId);
    if (!drag?.encoder) return;
    const detents = Math.trunc((drag.y - e.clientY) / 8);
    if (Math.abs(e.clientY - drag.y) > 3) drag.moved = true;
    if (detents) {
      spin(drag.encoder, detents);
      void props
        .runtime()
        ?.turn(drag.encoder, detents)
        ?.catch((error) => props.onError(String(error)));
      drag.y -= detents * 8;
    }
  };
  const dragEnd = (e: PointerEvent) => {
    releasePointer(e.pointerId);
  };
  const detachDrag = () => {
    window.removeEventListener('pointermove', dragMove);
    window.removeEventListener('pointerup', dragEnd);
    window.removeEventListener('pointercancel', dragEnd);
  };
  const releasePointer = (id: number) => {
    const state = pointers.get(id);
    if (!state) return;
    if (state.target && (state.moved || state.pushed)) {
      suppressedKnob = state.target;
      if (clearSuppression) window.clearTimeout(clearSuppression);
      clearSuppression = window.setTimeout(() => {
        suppressedKnob = undefined;
      }, 0);
    }
    if (state.code) emit(state.code, false);
    if (state.pushed) emit(state.pushed, false);
    if (funcLatched && (state.code || state.pushed)) {
      funcLatched = false;
      emit(FUNC, false);
    }
    pointers.delete(id);
    if (![...pointers.values()].some((p) => p.encoder)) detachDrag();
  };
  const releaseAll = () => {
    funcLatched = false;
    for (const code of pressed()) props.runtime()?.release(code);
    owners.clear();
    setPressed(new Set<number>());
    pointers.clear();
    detachDrag();
    suppressedKnob = undefined;
  };
  onMount(() => window.addEventListener('blur', releaseAll));
  onCleanup(() => {
    window.removeEventListener('blur', releaseAll);
    if (clearSuppression) window.clearTimeout(clearSuppression);
    detachDrag();
  });
  // oxlint-disable-next-line solid/reactivity -- the API is handed over once, at creation
  props.ref?.({
    show: (snapshot) => {
      if (snapshot.frame && canvas) drawFrame(canvas, snapshot.frame);
      if (snapshot.leds !== undefined) setLeds(parseLeds(snapshot.leds));
    },
    showFrame: (frame) => {
      if (canvas) drawFrame(canvas, frame);
    },
    clear: () => {
      if (canvas) canvas.getContext('2d')?.clearRect(0, 0, 128, 64);
      setLeds(new Uint8Array(LED_COUNT));
    },
    releaseAll,
    previewLeds: (preview) => setLeds(new Uint8Array(preview)),
  });
  return (
    <PanelArt
      panel={props.panel}
      ledStyle={indicatorLight}
      keyLight={keyLight}
      isPressed={(code) => pressed().has(code)}
      angle={angle}
      onSvgPointerUp={(e) => releasePointer(e.pointerId)}
      onSvgPointerCancel={(e) => releasePointer(e.pointerId)}
      onSvgLostPointerCapture={(e) => {
        if (pointers.get(e.pointerId)?.code) releasePointer(e.pointerId);
      }}
      onKnobPointerDown={(knob, e) => {
        if (!knob.encoder) return;
        e.preventDefault();
        const pushed = e.shiftKey ? knob.push : undefined;
        if (pushed) emit(pushed, true);
        pointers.set(e.pointerId, {
          encoder: knob.encoder,
          y: e.clientY,
          pushed,
          moved: false,
          target: e.currentTarget,
        });
        detachDrag();
        window.addEventListener('pointermove', dragMove);
        window.addEventListener('pointerup', dragEnd);
        window.addEventListener('pointercancel', dragEnd);
      }}
      onKnobClick={(knob, e) => {
        if (suppressedKnob === e.currentTarget) {
          suppressedKnob = undefined;
          return;
        }
        if (knob.push) {
          emit(knob.push, true);
          // oxlint-disable-next-line solid/reactivity -- runs inside the knob's click handler (a PanelArt prop)
          queueMicrotask(() => emit(knob.push!, false));
        }
      }}
      onKnobWheel={(knob, e) => {
        if (!knob.encoder) return;
        e.preventDefault();
        const detents = e.shiftKey ? Math.sign(-e.deltaY) * 10 : Math.sign(-e.deltaY);
        if (detents) spin(knob.encoder, detents);
        props.runtime()?.turn(knob.encoder, detents);
      }}
      onKeyPointerDown={(key, e) => {
        if (key.code === FUNC) {
          funcLatched = !funcLatched;
          emit(FUNC, funcLatched);
          return;
        }
        e.currentTarget.setPointerCapture(e.pointerId);
        pointers.set(e.pointerId, { code: key.code, y: e.clientY, moved: false });
        emit(key.code, true);
      }}
    >
      <canvas
        ref={canvas}
        class="oled"
        width="128"
        height="64"
        aria-label="128 by 64 OLED display"
        style={{
          left: `${(props.panel.glass[0] / 215) * 100}%`,
          top: `${(props.panel.glass[1] / 176) * 100}%`,
          width: `${((props.panel.glass[2] - props.panel.glass[0]) / 215) * 100}%`,
          height: `${((props.panel.glass[3] - props.panel.glass[1]) / 176) * 100}%`,
        }}
      />
    </PanelArt>
  );
}
