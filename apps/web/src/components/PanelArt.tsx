import { For, Show, type JSX } from 'solid-js';
import type { Key, Knob, Panel } from '../data/faceplates';
import './panel.css';

const iconPaths: Record<string, string> = {
  perform: 'M8 17.5 V4 Q10 6 11 7 M13 5 H21 M13 10 H21 M13 15 H21 M13 20 H21',
  settings:
    'M12 2.5 L14.2 5.3 L17.7 4.3 L18.1 7.9 L21.5 9.2 L19.6 12 L21.5 14.8 L18.1 16.1 L17.7 19.7 L14.2 18.7 L12 21.5 L9.8 18.7 L6.3 19.7 L5.9 16.1 L2.5 14.8 L4.4 12 L2.5 9.2 L5.9 7.9 L6.3 4.3 L9.8 5.3 Z',
  samples: 'M3 10 V14 M6.6 7 V17 M10.2 4 V20 M13.8 8.5 V15.5 M17.4 5.5 V18.5 M21 3 V21',
  tempo: 'M5 21 L9.5 3 H14.5 L19 21 Z M11 16 L20 6',
  keyboard: 'M5 3 H19 Q21 3 21 5 V19 Q21 21 19 21 H5 Q3 21 3 19 V5 Q3 3 5 3 Z M7.5 14 V21 M12 14 V21 M16.5 14 V21',
  record: 'M12 4 A8 8 0 1 0 12 20 A8 8 0 0 0 12 4',
  play: 'M7 4 L19.5 12 L7 20 Z',
  stop: 'M8 4.5 H16 Q19.5 4.5 19.5 8 V16 Q19.5 19.5 16 19.5 H8 Q4.5 19.5 4.5 16 V8 Q4.5 4.5 8 4.5 Z',
  up: 'M6 18.5 L12 5 L18 18.5',
  down: 'M6 5.5 L12 19 L18 5.5',
  left: 'M17 5 L6.5 12 L17 19',
  right: 'M7 5 L17.5 12 L7 19',
  unison: 'M6.3 7 L11 16.5 M12 6.2 V16.5 M17.7 7 L13 16.5',
  arp: 'M8.4 18.5 V7 M18.4 16.5 V5',
  noteedit: 'M14 3.5 V18',
  plus: 'M12 5 V19 M5 12 H19',
  minus: 'M5 12 H19',
  // sound: three dots, drawn as circles below; routing: the letters FX
  sound: '',
  routing: 'M3 20 V4 H10.5 M3 12 H9.5 M13.5 4 L21.5 20 M21.5 4 L13.5 20',
  headphones: 'M4.5 15 V12 C4.5 2 19.5 2 19.5 12 V15',
};
function Icon(props: { name: string }) {
  return (
    <g class="icon" aria-hidden="true">
      <path class="icon-stroke" d={iconPaths[props.name] ?? ''} />
      <Show when={props.name === 'settings'}>
        <circle class="icon-stroke" cx="12" cy="12" r="3" />
      </Show>
      <Show when={props.name === 'perform'}>
        <circle class="icon-fill" cx="5.5" cy="17.5" r="2.6" />
      </Show>
      <Show when={props.name === 'arp'}>
        <circle class="icon-fill" cx="6" cy="18.5" r="2.6" />
        <circle class="icon-fill" cx="16" cy="16.5" r="2.6" />
        <path class="icon-fill" d="M8.4 5.8 L18.4 3.8 V7 L8.4 9 Z" />
      </Show>
      <Show when={props.name === 'noteedit'}>
        <circle class="icon-fill" cx="11.8" cy="19" r="2.5" />
        <circle class="icon-fill" cx="11.8" cy="13" r="1.5" />
        <circle class="icon-fill" cx="11.8" cy="9" r="1.5" />
        <circle class="icon-fill" cx="11.8" cy="5" r="1.5" />
      </Show>
      <Show when={props.name === 'unison'}>
        <circle class="icon-stroke" cx="5" cy="5" r="2.2" />
        <circle class="icon-stroke" cx="12" cy="4" r="2.2" />
        <circle class="icon-stroke" cx="19" cy="5" r="2.2" />
        <circle class="icon-fill" cx="12" cy="19" r="2.6" />
      </Show>
      <Show when={props.name === 'keyboard'}>
        <rect class="icon-fill" x="6" y="3" width="3" height="11" />
        <rect class="icon-fill" x="10.5" y="3" width="3" height="11" />
        <rect class="icon-fill" x="15" y="3" width="3" height="11" />
      </Show>
      <Show when={props.name === 'sound'}>
        <circle class="icon-fill" cx="4.5" cy="12" r="2.6" />
        <circle class="icon-fill" cx="12" cy="12" r="2.6" />
        <circle class="icon-fill" cx="19.5" cy="12" r="2.6" />
      </Show>
      <Show when={props.name === 'headphones'}>
        <rect class="icon-fill" x="3" y="13.5" width="4.5" height="7.5" />
        <rect class="icon-fill" x="16.5" y="13.5" width="4.5" height="7.5" />
      </Show>
    </g>
  );
}

export interface PanelArtProps {
  panel: Panel;
  ledStyle?: (index: number) => JSX.CSSProperties | string | undefined;
  keyLight?: (key: Key) => JSX.CSSProperties | string | undefined;
  isPressed?: (code: number) => boolean;
  angle?: (encoder: number | undefined) => number;
  onSvgPointerUp?: (e: PointerEvent) => void;
  onSvgPointerCancel?: (e: PointerEvent) => void;
  onSvgLostPointerCapture?: (e: PointerEvent) => void;
  onKnobPointerDown?: (knob: Knob, e: PointerEvent & { currentTarget: SVGGElement }) => void;
  onKnobClick?: (knob: Knob, e: MouseEvent & { currentTarget: SVGGElement }) => void;
  onKnobWheel?: (knob: Knob, e: WheelEvent & { currentTarget: SVGGElement }) => void;
  onKeyPointerDown?: (key: Key, e: PointerEvent & { currentTarget: SVGGElement }) => void;
  children?: JSX.Element;
}

/** The device front panel drawing: no runtime dependency, so a page can render it without the emulator. */
export default function PanelArt(props: PanelArtProps) {
  const panel = () => props.panel;
  const ledStyle = (index: number) => props.ledStyle?.(index);
  const keyLight = (key: Key) => props.keyLight?.(key);
  const angle = (encoder: number | undefined) => props.angle?.(encoder) ?? 0;
  return (
    <div class="panel-surface">
      <svg
        class="panel"
        viewBox="0 0 215 176"
        onPointerUp={(e) => props.onSvgPointerUp?.(e)}
        onPointerCancel={(e) => props.onSvgPointerCancel?.(e)}
        onLostPointerCapture={(e) => props.onSvgLostPointerCapture?.(e)}
      >
        <defs>
          <linearGradient id="chassis" x2="0" y2="1">
            <stop stop-color="#34363a" />
            <stop offset=".5" stop-color="#2c2e31" />
            <stop offset="1" stop-color="#242528" />
          </linearGradient>
          <radialGradient id="knob">
            <stop stop-color="#44464b" />
            <stop offset=".68" stop-color="#2a2c2f" />
            <stop offset=".72" stop-color="#111214" />
          </radialGradient>
        </defs>
        <rect class="chassis" x="1" y="1" width="213" height="174" rx="3" />
        <For each={panel().screws}>
          {(s) => (
            <g class="screw">
              <circle cx={s.x} cy={s.y} r="2" />
              <path d={`M${s.x - 1.1} ${s.y}h2.2`} />
            </g>
          )}
        </For>
        <For each={panel().leds.y}>
          {(y, row) => (
            <For each={panel().leds.x}>
              {(x, col) => {
                const style = () => ledStyle(row() * panel().leds.x.length + col());
                return (
                  <circle class={`led ${style() ? 'lit' : ''}`} style={style()} cx={x} cy={y} r={panel().leds.r} />
                );
              }}
            </For>
          )}
        </For>
        <For each={panel().leds.extra ?? []}>
          {(led, i) => {
            const style = () => ledStyle(panel().leds.y.length * panel().leds.x.length + i());
            return (
              <circle
                class={`led ${style() ? 'lit' : ''}`}
                style={style()}
                cx={led.x}
                cy={led.y}
                r={led.r ?? panel().leds.r}
              />
            );
          }}
        </For>
        <For each={panel().texts}>
          {(t) =>
            t.style === 'icon' ? (
              <g class="panel-icon" transform={`translate(${t.x - 1.7} ${t.y - 1.7}) scale(.142)`}>
                <Icon name="headphones" />
              </g>
            ) : t.style === 'updown' ? (
              <>
                <path
                  class="silk-glyph"
                  d={`M${t.x - t.text.length * 0.55 - 2.6} ${t.y - 1.6} l.9 -1 l.9 1 M${t.x - t.text.length * 0.55 - 1.7} ${t.y - 2.6} v3.2 M${t.x - t.text.length * 0.55 - 2.6} ${t.y - 0.4} l.9 1 l.9 -1`}
                />
                <text class="silk label" x={t.x} y={t.y}>
                  {t.text}
                </text>
              </>
            ) : t.style === 'split' ? (
              <text class="silk" x={t.x} y={t.y}>
                {t.text.split('/')[0]}
                <tspan class="silk-accent">
                  {t.text.includes('/') ? `/${t.text.slice(t.text.indexOf('/') + 1)}` : ''}
                </tspan>
              </text>
            ) : (
              <text class={`silk ${t.style}`} x={t.x} y={t.y}>
                {t.text}
              </text>
            )
          }
        </For>
        <rect
          class="bezel"
          x={panel().bezel[0]}
          y={panel().bezel[1]}
          width={panel().bezel[2] - panel().bezel[0]}
          height={panel().bezel[3] - panel().bezel[1]}
          rx="1.5"
        />
        <rect
          class="glass"
          x={panel().glass[0]}
          y={panel().glass[1]}
          width={panel().glass[2] - panel().glass[0]}
          height={panel().glass[3] - panel().glass[1]}
          rx=".5"
        />
        <text class="wordmark" x={panel().wordmark[0]} y={panel().wordmark[1]}>
          digikit
        </text>
        <For each={panel().knobs}>
          {(knob) => (
            <g
              class="knob"
              onPointerDown={(e) => props.onKnobPointerDown?.(knob, e)}
              onClick={(e) => props.onKnobClick?.(knob, e)}
              onWheel={(e) => props.onKnobWheel?.(knob, e)}
            >
              <circle cx={knob.x + 0.65} cy={knob.y + 1} r={knob.r} class="knob-shadow" />
              <circle cx={knob.x} cy={knob.y} r={knob.r} class="knob-face" />
              <circle cx={knob.x} cy={knob.y} r={knob.r * 0.84} class="knob-top" />
              <g transform={`rotate(${angle(knob.encoder)} ${knob.x} ${knob.y})`}>
                <Show when={knob.marker === 'dot'}>
                  <circle class="knob-dot" cx={knob.x + knob.r * 0.58} cy={knob.y} r={knob.r * 0.12} />
                </Show>
                <Show when={knob.marker !== 'dot'}>
                  <path d={`M${knob.x} ${knob.y - knob.r * 0.78}v${knob.r * 0.33}`} />
                </Show>
              </g>
              <text x={knob.x} y={knob.label_y ?? knob.y + knob.r + 3.1}>
                {knob.label}
              </text>
              <Show when={knob.sub}>
                <text
                  class={`knob-sub ${knob.sub_style === 'accent' ? 'accent' : ''}`}
                  x={knob.x}
                  y={(knob.label_y ?? knob.y + knob.r + 3.1) + 2.4}
                >
                  {knob.sub}
                </text>
              </Show>
            </g>
          )}
        </For>
        <For each={panel().keys}>
          {(key) => (
            <g
              class={`key ${key.style ?? ''} ${key.icon ? `icon-${key.icon}` : ''} ${props.isPressed?.(key.code) ? 'down' : ''} ${keyLight(key) ? 'lit' : ''}`}
              style={keyLight(key)}
              onPointerDown={(e) => props.onKeyPointerDown?.(key, e)}
            >
              <rect
                x={key.x - key.w / 2 + 0.8}
                y={key.y - key.h / 2 + 1.2}
                width={key.w - 1.6}
                height={key.h - 1.2}
                rx="1.5"
                class="key-shadow"
              />
              <rect
                x={key.x - key.w / 2}
                y={key.y - key.h / 2}
                width={key.w}
                height={key.h}
                rx="1.5"
                class="key-body"
              />
              <rect
                x={key.x - key.w / 2 + 0.7}
                y={key.y - key.h / 2 + 0.56}
                width={key.w - 1.4}
                height={key.h - 1.68}
                rx="1.1"
                class="key-face"
              />
              <Show when={key.legend}>
                <text class="key-legend" x={key.x} y={key.y + 0.4}>
                  {key.legend}
                </text>
              </Show>
              <Show when={key.style === 'trig'}>
                <rect class="trig-underline" x={key.x - 2.4} y={key.y + 2.9} width="4.8" height=".7" rx=".15" />
              </Show>
              <Show when={key.icon}>
                <g transform={`translate(${key.x - 3} ${key.y - 3}) scale(.25)`}>
                  <Icon name={key.icon!} />
                </g>
              </Show>
              <Show when={key.sub}>
                <text class="key-sub" x={key.x} y={key.sub_y ?? key.y + key.h / 2 + 3.5}>
                  {key.sub}
                </text>
              </Show>
            </g>
          )}
        </For>
      </svg>
      {props.children}
    </div>
  );
}
