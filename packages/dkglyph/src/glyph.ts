import type { Raster } from './raster.ts';

/** A size in pixels, or the name of a class in the set's [sizes] table. */
export type GlyphSize = readonly [number, number] | string;

interface GlyphCommon {
  size: GlyphSize;
  title?: string;
  tags?: string[];
  /** Whether the glyph is meant to be shown inverted on a selection bar (default true). */
  invertible?: boolean;
}

export interface StaticGlyphDef extends GlyphCommon {
  /** Rows of '#' and '.'; whitespace lines are trimmed. */
  pixels?: string;
  draw?: (p: Raster) => void;
}

export interface ValueGlyphDef extends GlyphCommon {
  range: 'unipolar' | 'bipolar';
  frames: number;
  /** v is in [0,1] for unipolar and in [-1,1] for bipolar. */
  draw: (p: Raster, v: number) => void;
}

export interface TransitionDef {
  frames: number;
  /** t is in (0,1) and runs from state `from` to state `to`. */
  draw: (p: Raster, from: number, to: number, t: number) => void;
}

export interface StatesGlyphDef extends GlyphCommon {
  states: string[];
  draw?: (p: Raster, i: number) => void;
  pixels?: string[];
  transition?: TransitionDef;
}

export type GlyphDef = StaticGlyphDef | ValueGlyphDef | StatesGlyphDef;
export type GlyphKind = 'static' | 'value' | 'states';

export const kindOf = (def: GlyphDef): GlyphKind => ('range' in def ? 'value' : 'states' in def ? 'states' : 'static');

/** Declare a glyph. Returns the definition unchanged; it only types the file and rejects a shapeless one. */
export function glyph<T extends GlyphDef>(def: T): T {
  if (typeof def !== 'object' || def === null) throw new Error('a glyph definition must be an object');
  if (!def.size) throw new Error('a glyph needs a size: [w, h] or a size class name');
  return def;
}
