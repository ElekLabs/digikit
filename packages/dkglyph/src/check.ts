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

function glyphMessages(
  g: LoadedGlyph,
  margins: Readonly<Record<string, number>>,
  errors: Message[],
  warnings: Message[],
): void {
  const err = (text: string) => errors.push({ id: g.id, text });
  if (g.error || !g.rendered || !g.def) {
    err(`${g.error?.message ?? 'did not load'}${g.error ? ` (${g.error.where})` : ''}`);
    return;
  }
  const r = g.rendered;
  for (const p of r.problems) err(p);
  if (r.clipped > 0) err(`${r.clipped} pixel${r.clipped === 1 ? '' : 's'} drawn outside the ${r.w}x${r.h} raster`);
  if (r.kind === 'value' && !(Number.isInteger(r.declaredFrames) && (r.declaredFrames ?? 0) >= 2))
    err(`a value glyph needs an integer frame count of at least 2, has ${r.declaredFrames}`);
  const all = [...r.frames, ...(r.transitions ?? []).flatMap((t) => t.frames)];
  if (all.some((f) => f.count() === 0)) err('a frame is empty');
  const margin = typeof g.def.size === 'string' ? (margins[g.def.size] ?? 1) : 1;
  if (g.def.invertible !== false && margin > 0 && all.some((f) => touchesMargin(f, margin)))
    warnings.push({
      id: g.id,
      text: `invertible glyph touches its ${margin}-pixel outer margin, so the selection bar leaves no border`,
    });
}

function touchesMargin(f: Raster, m: number): boolean {
  for (let y = 0; y < f.h; y += 1)
    for (let x = 0; x < f.w; x += 1) if ((x < m || y < m || x >= f.w - m || y >= f.h - m) && f.get(x, y)) return true;
  return false;
}

export function checkSet(set: LoadedSet): CheckReport {
  const errors: Message[] = [];
  const warnings: Message[] = [];
  for (const text of set.errors) errors.push({ id: '(set)', text });
  const seen = new Set<string>();
  for (const g of set.glyphs) {
    seen.add(g.id);
    glyphMessages(g, set.margins, errors, warnings);
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
