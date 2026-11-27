import {
  type GlyphDef,
  type GlyphKind,
  kindOf,
  type StatesGlyphDef,
  type StaticGlyphDef,
  type ValueGlyphDef,
} from './glyph.ts';
import { Raster } from './raster.ts';

export interface RenderedTransition {
  from: number;
  to: number;
  frames: Raster[];
}

export interface RenderedGlyph {
  kind: GlyphKind;
  w: number;
  h: number;
  frames: Raster[];
  /** value glyphs: for each of the N samples, the index into `frames` after merging equal neighbours */
  valueMap?: number[];
  range?: 'unipolar' | 'bipolar';
  /** value glyphs: the declared frame count before merging */
  declaredFrames?: number;
  states?: string[];
  transitions?: RenderedTransition[];
  /** pixels placed outside the raster by draw calls, over all frames */
  clipped: number;
  /** problems found while rendering that check reports as errors */
  problems: string[];
}

export type SizeClasses = Readonly<Record<string, readonly [number, number]>>;

export function resolveSize(def: GlyphDef, classes: SizeClasses): readonly [number, number] {
  if (typeof def.size === 'string') {
    const found = classes[def.size];
    if (!found) throw new Error(`unknown size class "${def.size}"`);
    return found;
  }
  const [w, h] = def.size;
  if (!Number.isInteger(w) || !Number.isInteger(h) || w < 1 || h < 1) throw new Error(`bad size ${w}x${h}`);
  return [w, h];
}

/** Rows of a pixel template, whitespace lines dropped and each row trimmed. */
export function pixelRows(template: string): string[] {
  return template
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
}

function fromPixels(template: string, w: number, h: number, what: string, problems: string[]): Raster {
  const rows = pixelRows(template);
  if (rows.length !== h) problems.push(`${what}: ${rows.length} pixel rows, size says ${h}`);
  const r = new Raster(w, h);
  rows.forEach((row, y) => {
    if (row.length !== w) problems.push(`${what}: row ${y} is ${row.length} wide, size says ${w}`);
    if (/[^#.]/.test(row)) problems.push(`${what}: row ${y} has a character other than '#' or '.'`);
    for (let x = 0; x < Math.min(w, row.length); x += 1) if (row[x] === '#' && y < h) r.px(x, y);
  });
  return r;
}

/** Equal neighbours collapse to one frame; returns the kept frames and, per input, the kept index. */
function dedupe(all: Raster[]): { frames: Raster[]; map: number[] } {
  const frames: Raster[] = [];
  const map: number[] = [];
  for (const f of all) {
    const last = frames[frames.length - 1];
    if (!last || !last.equals(f)) frames.push(f);
    map.push(frames.length - 1);
  }
  return { frames, map };
}

function drawOne(w: number, h: number, draw: (p: Raster) => void): Raster {
  const p = new Raster(w, h);
  draw(p);
  return p;
}

const sum = (rs: Raster[]) => rs.reduce((n, r) => n + r.clipped, 0);

function renderStatic(def: StaticGlyphDef, w: number, h: number): RenderedGlyph {
  const problems: string[] = [];
  const p = def.draw
    ? drawOne(w, h, def.draw)
    : def.pixels !== undefined
      ? fromPixels(def.pixels, w, h, 'pixels', problems)
      : (problems.push('a static glyph needs pixels or draw'), new Raster(w, h));
  return { kind: 'static', w, h, frames: [p], clipped: p.clipped, problems };
}

function renderValue(def: ValueGlyphDef, w: number, h: number): RenderedGlyph {
  const problems: string[] = [];
  // an invalid count renders no frames; check reports it
  const n = Number.isInteger(def.frames) && def.frames >= 1 ? def.frames : 0;
  const [lo, hi] = def.range === 'bipolar' ? [-1, 1] : [0, 1];
  const all: Raster[] = [];
  for (let i = 0; i < n; i += 1) {
    const v = n === 1 ? lo : lo + ((hi - lo) * i) / (n - 1);
    all.push(drawOne(w, h, (p) => def.draw(p, v)));
  }
  const { frames, map } = dedupe(all);
  return {
    kind: 'value',
    w,
    h,
    frames,
    valueMap: map,
    range: def.range,
    declaredFrames: def.frames,
    clipped: sum(all),
    problems,
  };
}

function renderStates(def: StatesGlyphDef, w: number, h: number): RenderedGlyph {
  const problems: string[] = [];
  const frames = def.states.map((name, i) => {
    const px = def.pixels?.[i];
    if (px !== undefined) return fromPixels(px, w, h, `state ${name}`, problems);
    if (def.draw) return drawOne(w, h, (p) => def.draw!(p, i));
    problems.push(`state ${name} has neither pixels nor draw`);
    return new Raster(w, h);
  });
  const transitions: RenderedTransition[] = [];
  const tr = def.transition;
  if (tr) {
    const wanted: [number, number][] = [];
    if (tr.pairs) {
      for (const [a, b] of tr.pairs) {
        const from = def.states.indexOf(a);
        const to = def.states.indexOf(b);
        if (from < 0 || to < 0) problems.push(`transition pair [${a}, ${b}] names an unknown state`);
        else if (from !== to) wanted.push([from, to]);
      }
    } else
      for (let from = 0; from < frames.length; from += 1)
        for (let to = 0; to < frames.length; to += 1) if (from !== to) wanted.push([from, to]);
    const trFrames = Number.isInteger(tr.frames) && tr.frames >= 1 ? tr.frames : 0;
    if (trFrames === 0) problems.push(`transition frames must be an integer of at least 1, has ${tr.frames}`);
    for (const [from, to] of wanted) {
      const list: Raster[] = [];
      for (let k = 1; k <= trFrames; k += 1) list.push(drawOne(w, h, (p) => tr.draw(p, from, to, k / (trFrames + 1))));
      transitions.push({ from, to, frames: list });
    }
  }
  const clipped = sum(frames) + transitions.reduce((n, t) => n + sum(t.frames), 0);
  return { kind: 'states', w, h, frames, states: [...def.states], transitions, clipped, problems };
}

/** Render a definition into frames. Throws when the size cannot be resolved or a draw function throws. */
export function renderGlyph(def: GlyphDef, classes: SizeClasses = {}): RenderedGlyph {
  const [w, h] = resolveSize(def, classes);
  const kind = kindOf(def);
  if (kind === 'value') return renderValue(def as ValueGlyphDef, w, h);
  if (kind === 'states') return renderStates(def as StatesGlyphDef, w, h);
  return renderStatic(def as StaticGlyphDef, w, h);
}
