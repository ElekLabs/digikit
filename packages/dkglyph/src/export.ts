import { createCanvas } from '@napi-rs/canvas';
import type { Raster } from './raster.ts';
import type { LoadedSet } from './set.ts';

export interface GlyphJson {
  id: string;
  group: string;
  title: string;
  tags: string[];
  invertible: boolean;
  kind: 'static' | 'value' | 'states';
  w: number;
  h: number;
  /** one hex string per frame: w*h bits, row-major, MSB first, padded with zero bits to a whole byte */
  frames: string[];
  range?: 'unipolar' | 'bipolar';
  valueMap?: number[];
  states?: string[];
  transitions?: Array<{ from: number; to: number; frames: string[] }>;
}

export interface SetJson {
  name: string;
  title: string;
  description: string;
  glyphs: GlyphJson[];
}

export const ON = [242, 244, 249] as const;
export const OFF = [8, 9, 12] as const;
const rgb = (c: readonly number[]) => `rgb(${c[0]},${c[1]},${c[2]})`;

export function hexOf(r: Raster): string {
  const bytes = new Uint8Array(Math.ceil((r.w * r.h) / 8));
  for (let i = 0; i < r.w * r.h; i += 1) if (r.get(i % r.w, Math.floor(i / r.w))) bytes[i >> 3]! |= 0x80 >> (i & 7);
  return Buffer.from(bytes).toString('hex');
}

/** The built set as plain data; glyphs that failed to load are left out. */
export function setJson(set: LoadedSet): SetJson {
  const glyphs: GlyphJson[] = [];
  for (const g of set.glyphs) {
    if (!g.rendered || !g.def) continue;
    const r = g.rendered;
    glyphs.push({
      id: g.id,
      group: g.group,
      title: g.def.title ?? g.id,
      tags: g.def.tags ?? [],
      invertible: g.def.invertible !== false,
      kind: r.kind,
      w: r.w,
      h: r.h,
      frames: r.frames.map(hexOf),
      ...(r.range ? { range: r.range } : {}),
      ...(r.valueMap ? { valueMap: r.valueMap } : {}),
      ...(r.states ? { states: r.states } : {}),
      ...(r.transitions
        ? { transitions: r.transitions.map((t) => ({ from: t.from, to: t.to, frames: t.frames.map(hexOf) })) }
        : {}),
    });
  }
  return { name: set.name, title: set.title, description: set.description, glyphs };
}

const SCALE = 4;
const GAP = 1;
const PAD = 8;
const LABEL = 14;
const MAX_SHOWN = 5;

/** The frames a sheet shows for one glyph: all of a static or states glyph, a spread of a value glyph. */
function shownFrames(frames: Raster[]): Raster[] {
  if (frames.length <= MAX_SHOWN) return frames;
  return Array.from({ length: MAX_SHOWN }, (_, i) => frames[Math.round((i * (frames.length - 1)) / (MAX_SHOWN - 1))]!);
}

/** PNG contact sheet: one row per group, each glyph at 4x with 1 px grid gaps between pixels. */
export function sheetPng(set: LoadedSet): Buffer {
  const groups = new Map<string, typeof set.glyphs>();
  for (const g of set.glyphs) if (g.rendered) groups.set(g.group, [...(groups.get(g.group) ?? []), g]);
  const cellW = (r: Raster) => r.w * (SCALE + GAP) + GAP;
  const cellH = (r: Raster) => r.h * (SCALE + GAP) + GAP;
  let width = 2 * PAD + 120;
  let height = PAD;
  const rowsInfo = [...groups.entries()].map(([name, list]) => {
    let w = 0;
    let h = 0;
    for (const g of list) {
      const fr = shownFrames(g.rendered!.frames);
      w += fr.length * (cellW(fr[0]!) + PAD);
      h = Math.max(h, cellH(fr[0]!));
    }
    width = Math.max(width, 2 * PAD + 100 + w);
    const top = height;
    height += h + LABEL + PAD;
    return { name, list, top, h };
  });
  const canvas = createCanvas(width, Math.max(height, 2 * PAD));
  const ctx = canvas.getContext('2d');
  ctx.fillStyle = '#16171c';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.font = '11px sans-serif';
  for (const row of rowsInfo) {
    ctx.fillStyle = '#9aa0b0';
    ctx.fillText(row.name, PAD, row.top + 12);
    let x = PAD + 100;
    for (const g of row.list) {
      const fr = shownFrames(g.rendered!.frames);
      const x0 = x;
      for (const f of fr) {
        ctx.fillStyle = rgb(OFF);
        ctx.fillRect(x, row.top, cellW(f), cellH(f));
        for (let py = 0; py < f.h; py += 1)
          for (let px = 0; px < f.w; px += 1) {
            if (!f.get(px, py)) continue;
            ctx.fillStyle = rgb(ON);
            ctx.fillRect(x + GAP + px * (SCALE + GAP), row.top + GAP + py * (SCALE + GAP), SCALE, SCALE);
          }
        x += cellW(f) + PAD;
      }
      ctx.fillStyle = '#9aa0b0';
      ctx.fillText(g.id.split('/').slice(1).join('/') || g.id, x0, row.top + row.h + 11);
    }
  }
  return canvas.toBuffer('image/png');
}
