import type { GlyphJson } from '../src/export.ts';
import { canvasOf, decode, drawBits, frameFor } from './bitmap.ts';
import { el } from './element.ts';
import { MENU, PAGE, PAGE_VALUES, SCREEN } from './layout.ts';
import { OFF, ON } from './palette.ts';

/** Position (0..1, or state index) shown for the k-th instance of a glyph on the mock page. */
function instancePosition(g: GlyphJson, k: number): number {
  if (g.kind === 'states') return k % (g.states?.length ?? 1);
  return PAGE_VALUES[k]!;
}

/** A 128x64 screen at 4x with a mock parameter page and a mock menu row. The layout is preview only (layout.ts). */
export function renderContext(root: HTMLElement, g: GlyphJson | undefined): void {
  root.replaceChildren();
  if (!g) return;
  const canvas = canvasOf(SCREEN.w, SCREEN.h, SCREEN.scale);
  const ctx = canvas.getContext('2d')!;
  ctx.fillStyle = OFF;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  const s = SCREEN.scale;
  const put = (hex: string, x: number, y: number, inverted: boolean) =>
    drawBits(ctx, decode(hex, g.w, g.h), g.w, g.h, x * s, y * s, { scale: s, grid: false, inverted });
  const bar = (x: number, y: number, w: number) => {
    ctx.fillStyle = ON;
    ctx.fillRect(x * s, y * s, w * s, s);
  };

  // parameter page: four slots, a text placeholder under each
  PAGE.slotX.forEach((x, k) => {
    put(frameFor(g, instancePosition(g, k)), x + Math.floor((PAGE.slotW - g.w) / 2), PAGE.glyphTop, false);
    bar(x + 2, PAGE.labelTop, PAGE.slotW - 4);
  });

  // menu row: inverted selection bar with the glyph as its icon
  ctx.fillStyle = ON;
  ctx.fillRect(0, MENU.top * s, SCREEN.w * s, MENU.rowH * s);
  put(frameFor(g, 0), MENU.iconX, MENU.top + Math.floor((MENU.rowH - g.h) / 2), g.invertible);
  ctx.fillStyle = OFF;
  ctx.fillRect(MENU.textX * s, (MENU.top + 6) * s, 50 * s, s);

  root.append(el('h2', {}, 'Context (preview layout)'), canvas);
}
