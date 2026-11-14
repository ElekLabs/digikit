// Direct imports preserve every authored faceplate field from the repository TOML.
// Vendored byte-for-byte from the faceplate source at commit 858140f.
import { parse } from 'smol-toml';
import dt2Source from './faceplates/digitakt-ii.toml?raw';
import dn2Source from './faceplates/digitone-ii.toml?raw';
// Key code -> LED index, from the firmware sweep (devices/*-leds.toml, tools/emulator/gen_led_toml.py).
import dt2Leds from '../../../../devices/digitakt-ii-leds.toml?raw';
import dn2Leds from '../../../../devices/digitone-ii-leds.toml?raw';

export type Key = {
  code: number;
  x: number;
  y: number;
  w: number;
  h: number;
  legend?: string;
  sub?: string;
  sub_y?: number;
  sub_style?: string;
  icon?: string;
  style?: string;
  /** Panel LED index (UART8 `b9 idx val`). Defaults to the key code for trig keys; unset = never lit. */ led?: number;
  /** CSS colour of the lit legend. */ led_color?: string;
};
export type Knob = {
  encoder?: number;
  push?: number;
  x: number;
  y: number;
  r: number;
  marker?: string;
  label: string;
  label_y?: number;
  sub?: string;
};
export type Text = { text: string; x: number; y: number; style: string };
export type Panel = {
  name: string;
  accent: string;
  func_legend: string;
  bezel: number[];
  glass: number[];
  wordmark: number[];
  leds: { x: number[]; y: number[]; r: number; idx?: number[]; color?: string };
  screws: { x: number; y: number }[];
  keys: Key[];
  knobs: Knob[];
  texts: Text[];
};
type Source = {
  faceplate: { wordmark: string; accent: string; func_legend: string };
  display: { bezel: number[]; glass: number[]; wordmark: number[] };
  leds: Panel['leds'];
  controls: { screws: Panel['screws']; keys: Key[]; knobs: Knob[]; texts: Text[] };
};
type LedMap = { key?: { code: number; led?: number }[]; indicator?: { led: number }[] };
function panel(name: string, source: string, ledSource: string): Panel {
  const data = parse(source) as unknown as Source,
    map = parse(ledSource) as unknown as LedMap;
  const byCode = new Map((map.key ?? []).filter((k) => k.led !== undefined).map((k) => [k.code, k.led as number]));
  const keys = data.controls.keys.map((key) => ({ ...key, led: key.led ?? byCode.get(key.code) }));
  // The 8 circles beside the display are the PAGE1..8 LEDs, in reading order.
  const idx = (map.indicator ?? []).map((i) => i.led);
  const leds = {
    ...data.leds,
    idx: data.leds.idx ?? (idx.length === data.leds.x.length * data.leds.y.length ? idx : undefined),
  };
  return {
    name,
    accent: data.faceplate.accent,
    func_legend: data.faceplate.func_legend,
    bezel: data.display.bezel,
    glass: data.display.glass,
    wordmark: data.display.wordmark,
    leds,
    ...data.controls,
    keys,
  };
}
export const panels = {
  dt2: panel('Digitakt II', dt2Source, dt2Leds),
  dn2: panel('Digitone II', dn2Source, dn2Leds),
} as const;
