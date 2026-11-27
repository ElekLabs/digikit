import type { Raster } from './raster.ts';
import type { LoadedGlyph, LoadedSet } from './set.ts';

export interface Message {
  id: string;
  text: string;
}

export interface CheckReport {
  errors: Message[];
  warnings: Message[];
  /** catalogue ids with no glyph file */
  missing: string[];
  /** catalogue ids marked v1 with no glyph file */
  missingV1: string[];
}

function glyphMessages(g: LoadedGlyph, errors: Message[], warnings: Message[]): void {
  const err = (text: string) => errors.push({ id: g.id, text });
  if (g.error || !g.rendered || !g.def) {
    err(`${g.error?.message ?? 'did not load'}${g.error ? ` (${g.error.where})` : ''}`);
    return;
  }
  const r = g.rendered;
  for (const p of r.problems) err(p);
  if (r.clipped > 0) err(`${r.clipped} pixel${r.clipped === 1 ? '' : 's'} drawn outside the ${r.w}x${r.h} raster`);
  if (r.kind === 'value' && (r.declaredFrames ?? 0) < 2)
    err(`a value glyph needs at least 2 frames, has ${r.declaredFrames}`);
  const all = [...r.frames, ...(r.transitions ?? []).flatMap((t) => t.frames)];
  if (all.some((f) => f.count() === 0)) err('a frame is empty');
  if (g.def.invertible !== false && all.some((f) => touchesMargin(f)))
    warnings.push({
      id: g.id,
      text: 'invertible glyph touches its 1-pixel outer margin, so the selection bar leaves no border',
    });
}

function touchesMargin(f: Raster): boolean {
  for (let x = 0; x < f.w; x += 1) if (f.get(x, 0) || f.get(x, f.h - 1)) return true;
  for (let y = 0; y < f.h; y += 1) if (f.get(0, y) || f.get(f.w - 1, y)) return true;
  return false;
}

export function checkSet(set: LoadedSet): CheckReport {
  const errors: Message[] = [];
  const warnings: Message[] = [];
  const seen = new Set<string>();
  for (const g of set.glyphs) {
    if (seen.has(g.id)) errors.push({ id: g.id, text: 'duplicate glyph id' });
    seen.add(g.id);
    glyphMessages(g, errors, warnings);
  }
  const listed = new Set<string>();
  for (const c of set.catalogue) {
    if (listed.has(c.id)) errors.push({ id: c.id, text: 'duplicate catalogue id' });
    listed.add(c.id);
  }
  for (const g of set.glyphs)
    if (!listed.has(g.id)) warnings.push({ id: g.id, text: 'glyph id is missing from catalogue.toml' });
  const absent = set.catalogue.filter((c) => !seen.has(c.id));
  return {
    errors,
    warnings,
    missing: absent.map((c) => c.id),
    missingV1: absent.filter((c) => c.v1).map((c) => c.id),
  };
}
