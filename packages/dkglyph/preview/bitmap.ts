import type { GlyphJson } from '../src/export.ts';
import { OFF, ON } from './palette.ts';

/** Decode one hex frame (row-major, MSB first) into a 0/1 array. */
export function decode(hex: string, w: number, h: number): Uint8Array {
  const out = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i += 1) {
    const byte = parseInt(hex.slice((i >> 3) * 2, (i >> 3) * 2 + 2), 16);
    out[i] = (byte >> (7 - (i & 7))) & 1;
  }
  return out;
}

export interface DrawOptions {
  scale: number;
  grid: boolean;
  inverted: boolean;
}

/** Draw pixels into ctx at (x, y). Inverted swaps the two colours. */
export function drawBits(
  ctx: CanvasRenderingContext2D,
  bits: Uint8Array,
  w: number,
  h: number,
  x: number,
  y: number,
  o: DrawOptions,
): void {
  const [on, off] = o.inverted ? [OFF, ON] : [ON, OFF];
  ctx.fillStyle = off;
  ctx.fillRect(x, y, w * o.scale, h * o.scale);
  ctx.fillStyle = on;
  const gap = o.grid && o.scale >= 4 ? 1 : 0;
  for (let j = 0; j < h; j += 1)
    for (let i = 0; i < w; i += 1)
      if (bits[j * w + i]) ctx.fillRect(x + i * o.scale, y + j * o.scale, o.scale - gap, o.scale - gap);
}

/** The frame of a glyph shown at a position: v in 0..1 over the value range, or a state index. */
export function frameFor(g: GlyphJson, position: number): string {
  if (g.kind === 'value' && g.valueMap) {
    const idx = Math.round(Math.max(0, Math.min(1, position)) * (g.valueMap.length - 1));
    return g.frames[g.valueMap[idx]!]!;
  }
  if (g.kind === 'states') return g.frames[Math.min(g.frames.length - 1, Math.floor(position))]!;
  return g.frames[0]!;
}

export function canvasOf(w: number, h: number, scale: number): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = w * scale;
  c.height = h * scale;
  return c;
}
