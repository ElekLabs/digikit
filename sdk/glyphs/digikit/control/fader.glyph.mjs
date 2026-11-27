// Fader as the stock LEV: frame with cut corners, filled bar rising from the bottom, short scale ticks on both sides.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Fader',
  size: 'knob',
  range: 'unipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    for (const y of [0, 4, 8, 12, 16]) {
      p.line(0, y, 2, y);
      p.line(14, y, 16, y);
    }
    p.line(6, 0, 10, 0);
    p.line(6, 16, 10, 16);
    p.line(5, 1, 5, 15);
    p.line(11, 1, 11, 15);
    const h = Math.round(v * 13);
    if (h > 0) p.rect(7, 15 - h, 3, h);
  },
};
