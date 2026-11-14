// Front-panel LED values from the firmware's own panel records (packages/emulator/boot panel_leds.rs).
// Everything that turns a value into light lives in `ledLight`, so a corrected mapping is a one-function change.

export const LED_COUNT = 64;

export function parseLeds(hex: string): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(LED_COUNT);
  for (let i = 0; i < LED_COUNT && i * 2 + 1 < hex.length; i += 1) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export interface Light {
  on: boolean;
  intensity: number;
  /** CSS colour from the value's colour mask; undefined when the mask is empty (use a fallback). */ color?: string;
}

/**
 * One panel LED value (docs/findings/19-sequencer-and-leds.md, [value] in devices/*-leds.toml): 0x01 is off; bits 4-7
 * are a colour mask (0x10 red, 0x20 green, 0x40 blue, 0x80 a fourth channel, drawn warm white); the low nibble is the
 * mode: 1 steady, anything else (0x0a, 6, 2) a dim or pulsing variant, drawn at 35%. A lit value with an empty mask
 * (0x02 0x06 0x0a: page and selected-track LEDs) has no colour of its own.
 */
export function ledLight(value: number): Light {
  if (value <= 1) return { on: false, intensity: 0 };
  const mask = value >> 4,
    intensity = (value & 15) === 1 ? 1 : 0.35;
  if (mask === 0) return { on: true, intensity };
  const rgb = [mask & 1 ? 255 : 0, mask & 2 ? 255 : 0, mask & 4 ? 255 : 0];
  if (mask & 8) {
    rgb[0] = 255;
    rgb[1] = Math.max(rgb[1], 227);
    rgb[2] = Math.max(rgb[2], 166);
  }
  return { on: true, intensity, color: `rgb(${rgb.join(',')})` };
}

export const WARM_WHITE = '#ffe3a6';
/** Default colour for a key's lit legend: PLAY green, RECORD red, the rest warm white. */
export const defaultLedColor = (icon?: string) =>
  icon === 'play' ? '#46d160' : icon === 'record' ? '#ff5a4f' : WARM_WHITE;
