// Unipolar knob: a 270 degree arc that fills clockwise from the lower left (135 degrees) to the lower right (45 degrees),
// a centre dot and a pointer. Angles grow clockwise on the screen, 0 is right.
const START = 135;
const SWEEP = 270;

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Knob',
  size: 'knob',
  range: 'unipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    const a = START + SWEEP * v;
    if (v > 0) p.arc(6, 6, 6, START, a);
    const rad = (a * Math.PI) / 180;
    p.line(6, 6, 6 + 3 * Math.cos(rad), 6 + 3 * Math.sin(rad));
    p.px(6, 6);
  },
};
