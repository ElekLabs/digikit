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
export function renderContext(root: HTMLElement, g: GlyphJson | undefined, set?: { glyphs: GlyphJson[] }): void {
  root.replaceChildren();
  if (!g) return;
  const canvas = canvasOf(SCREEN.w, SCREEN.h, SCREEN.scale);
  const ctx = canvas.getContext('2d')!;
  ctx.fillStyle = OFF;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  const s = SCREEN.scale;
  const put = (hex: string, x: number, y: number, inverted: boolean) =>
    drawBits(ctx, decode(hex, g.w, g.h), g.w, g.h, x * s, y * s, { scale: s, grid: false, inverted });

  // parameter page: four knob slots, a text placeholder under each; a panel-class glyph spans the first two slots
  const label = (x: number) => {
    ctx.fillStyle = ON;
    ctx.fillRect((x + 2) * s, (PAGE.labelTop + 1) * s, (PAGE.slotW - 4) * s, PAGE.labelH * s);
  };
  if (g.w > PAGE.slotW) {
    put(frameFor(g, instancePosition(g, 0)), PAGE.slotX[0], PAGE.glyphTop, false);
    PAGE.slotX.forEach((x) => label(x));
  } else {
    PAGE.slotX.forEach((x, k) => {
      put(frameFor(g, instancePosition(g, k)), x + Math.floor((PAGE.slotW - g.w) / 2), PAGE.glyphTop, false);
      label(x);
    });
  }

  // menu row: inverted selection bar; the icon is the glyph if it fits, else the system/machine glyph
  ctx.fillStyle = ON;
  ctx.fillRect(MENU.barX0 * s, (MENU.top + 1) * s, (MENU.barX1 - MENU.barX0) * s, MENU.barH * s);
  let note = '';
  if (g.h <= MENU.barH) {
    put(frameFor(g, 0), MENU.iconX, MENU.top + 1 + Math.floor((MENU.barH - g.h) / 2), g.invertible);
  } else {
    note = 'menu row shows icons up to 11 px tall';
    const fb = set?.glyphs.find((o) => o.id === 'system/machine');
    if (fb && fb.h <= MENU.barH)
      drawBits(
        ctx,
        decode(frameFor(fb, 0), fb.w, fb.h),
        fb.w,
        fb.h,
        MENU.iconX * s,
        (MENU.top + 1 + Math.floor((MENU.barH - fb.h) / 2)) * s,
        {
          scale: s,
          grid: false,
          inverted: fb.invertible,
        },
      );
  }
  ctx.fillStyle = OFF;
  ctx.fillRect(MENU.textX * s, (MENU.top + 5) * s, 50 * s, s);

  root.append(el('h2', {}, 'Context (preview layout)'), canvas);
  if (note) root.append(el('div', { class: 'muted' }, note));
}
