// Pan: dotted track with end stops and a centre detent, a marker that moves from L to R.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Pan',
  size: 'knob',
  range: 'bipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    for (let x = 2; x <= 14; x += 2) p.px(x, 8);
    p.line(0, 6, 0, 10);
    p.line(16, 6, 16, 10);
    p.line(8, 5, 8, 11);
    const cx = 8 + Math.round(v * 5);
    p.rect(cx - 1, 5, 3, 7);
  },
};
