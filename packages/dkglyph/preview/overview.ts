import type { GlyphJson } from '../src/export.ts';
import { canvasOf, decode, drawBits, frameFor } from './bitmap.ts';
import { el } from './element.ts';
import type { ViewState } from './state.ts';

/** Position of an animated glyph at time t seconds: a sweep for values, cycling states otherwise. */
export function sweepPosition(g: GlyphJson, t: number): number {
  if (g.kind === 'value') {
    const x = (t / 2) % 1;
    return x < 0.5 ? x * 2 : 2 - x * 2;
  }
  if (g.kind === 'states') return Math.floor(t) % (g.states?.length ?? 1);
  return 0;
}

export function paintGlyph(canvas: HTMLCanvasElement, g: GlyphJson, position: number, s: ViewState): void {
  const ctx = canvas.getContext('2d')!;
  drawBits(ctx, decode(frameFor(g, position), g.w, g.h), g.w, g.h, 0, 0, {
    scale: s.scale,
    grid: s.grid,
    inverted: s.inverted,
  });
}

/** The overview: glyphs grouped by `group`. Returns the canvases that animate. */
export function renderOverview(
  root: HTMLElement,
  s: ViewState,
  select: (id: string) => void,
): Array<{ canvas: HTMLCanvasElement; glyph: GlyphJson }> {
  root.replaceChildren();
  const animated: Array<{ canvas: HTMLCanvasElement; glyph: GlyphJson }> = [];
  const groups = new Map<string, GlyphJson[]>();
  for (const g of s.set?.glyphs ?? []) groups.set(g.group, [...(groups.get(g.group) ?? []), g]);
  for (const [name, list] of groups) {
    const row = el('div', { class: 'grid' });
    for (const g of list) {
      const canvas = canvasOf(g.w, g.h, s.scale);
      paintGlyph(canvas, g, g.kind === 'value' ? 1 : 0, s);
      if (s.animate && g.kind !== 'static') animated.push({ canvas, glyph: g });
      row.append(
        el(
          'div',
          { class: `cell${g.id === s.selected ? ' selected' : ''}`, onclick: () => select(g.id) },
          canvas,
          g.id.split('/').slice(1).join('/'),
        ),
      );
    }
    root.append(el('h2', {}, name), row);
  }
  if (!groups.size) root.append(el('p', { class: 'muted' }, 'No glyphs in this set yet.'));
  return animated;
}
