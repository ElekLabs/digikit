// One period of a sine wave, plotted one sample per column so the stroke has no gaps.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StaticGlyphDef} */
export default {
  title: 'Sine',
  size: [15, 9],
  tags: ['wave'],
  draw(p) {
    p.plot((t) => Math.sin(2 * Math.PI * t), { x0: 1, y0: 1, w: 13, h: 7 });
  },
};
