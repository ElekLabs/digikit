// Tune: semitone ticks with a taller centre tick, a pointer that moves along them.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Tune',
  size: 'knob',
  range: 'bipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    for (let x = 2; x <= 14; x += 2) p.line(x, 12, x, 14);
    p.line(8, 10, 8, 15);
    const cx = 8 + Math.round(v * 6);
    p.line(cx - 2, 2, cx + 2, 2);
    p.line(cx - 1, 3, cx + 1, 3);
    p.px(cx, 4);
    p.line(cx, 5, cx, 10);
  },
};
