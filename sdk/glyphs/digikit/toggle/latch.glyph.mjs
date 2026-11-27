// Latch: momentary = key cap raised 1 px above its base with a spring arrow beside it; latch = cap pressed flush, a hook bar holding it.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Latch',
  size: 'icon',
  states: ['momentary', 'latch'],
  tags: ['toggle'],
  draw(p, i) {
    p.rect(0, 7, 9, 2);
    if (i === 0) {
      p.frame(0, 2, 6, 4);
      p.px(7, 0); p.px(6, 1); p.px(8, 1); p.px(7, 1); p.px(7, 2); p.px(7, 3); p.px(7, 4);
    } else {
      p.rect(0, 3, 6, 3);
      p.px(6, 3); p.px(7, 3); p.px(8, 3); p.px(8, 4); p.px(8, 5);
    }
  },
};
