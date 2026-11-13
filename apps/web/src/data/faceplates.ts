// A device has a faceplate when `src/data/faceplates/<stem>.toml` exists, where <stem> is the file name stem of its device file
// (`devices/<stem>.toml`, `DeviceRecord.stem`); the LED map `devices/<stem>-leds.toml` is optional. No faceplate file means no
// faceplate, not an error. The files are bundled as text and parsed here, so every authored faceplate field is kept.
import { parse } from 'smol-toml';
import { registry } from './device-profiles';

const faceplateFiles = import.meta.glob('./faceplates/*.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
// Key code -> LED index, from the firmware sweep (devices/*-leds.toml, tools/emulator/gen_led_toml.py).
const ledFiles = import.meta.glob('../../../../devices/*-leds.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const byStem = (files: Record<string, string>, suffix: string): Map<string, string> =>
  new Map(
    Object.entries(files).map(([path, text]) => [path.slice(path.lastIndexOf('/') + 1).replace(suffix, ''), text]),
  );

export type Key = {
  code: number;
  x: number;
  y: number;
  w: number;
  h: number;
  legend?: string;
  sub?: string;
  sub_y?: number;
  /** Reserved: a key's `sub` is always drawn in the accent colour. */ sub_style?: string;
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
  /** `accent` draws `sub` in the panel accent colour; unset draws it grey. */
  sub_style?: string;
};
/** A single LED outside the grid (an LED under a key). `idx` is its panel LED index; unset = never lit. */
export type ExtraLed = { x: number; y: number; r?: number; idx?: number; color?: string };
/**
 * The LED grid: the circles at every `x` of every `y`, row by row, and the single LEDs of `extra`. `idx[i]` is the panel LED index
 * of the i-th LED (the grid first, then `extra` in order); a hole or a missing array means the LED is never lit.
 */
export type Leds = {
  x: number[];
  y: number[];
  r: number;
  idx?: (number | undefined)[];
  color?: string;
  extra?: ExtraLed[];
};
export type Text = { text: string; x: number; y: number; style: string };
export type Panel = {
  name: string;
  accent: string;
  func_legend: string;
  bezel: number[];
  glass: number[];
  wordmark: number[];
  leds: Leds;
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
  const grid = data.leds.x.length * data.leds.y.length;
  const gridIdx: (number | undefined)[] | undefined = data.leds.idx ?? (idx.length === grid ? idx : undefined);
  const extra = data.leds.extra ?? [];
  const leds: Leds = {
    ...data.leds,
    idx: extra.length
      ? Array.from({ length: grid + extra.length }, (_, i) => (i < grid ? gridIdx?.[i] : extra[i - grid]?.idx))
      : gridIdx,
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
/** The faceplates by device id, for the devices of the registry that have a faceplate file. */
export const panels: Readonly<Record<string, Panel>> = (() => {
  const plates = byStem(faceplateFiles, '.toml');
  const leds = byStem(ledFiles, '-leds.toml');
  const out: Record<string, Panel> = {};
  for (const d of registry.devices) {
    const source = plates.get(d.stem);
    if (source !== undefined) out[d.id] = panel(d.name, source, leds.get(d.stem) ?? '');
  }
  return out;
})();

/** The devices that have a faceplate, in registry order. */
export const faceplateIds: readonly string[] = Object.keys(panels);
