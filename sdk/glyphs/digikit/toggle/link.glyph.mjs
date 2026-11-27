// Chain: unlinked = two separate square links; linked = two rounded links that overlap.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Link',
  size: 'icon',
  states: ['unlinked', 'linked'],
  tags: ['toggle'],
  draw(p, i) {
    if (i === 0) {
      p.frame(1, 3, 3, 3);
      p.frame(5, 3, 3, 3);
    } else {
      for (const x of [1, 3]) {
        p.line(x + 1, 3, x + 3, 3);
        p.line(x + 1, 5, x + 3, 5);
        p.px(x, 4);
        p.px(x + 4, 4);
      }
    }
  },
};
