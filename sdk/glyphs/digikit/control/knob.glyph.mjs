// Stock knob: 1 px ring (centre 8,8 radius 8), pointer from the centre to one pixel inside the ring.
// Sweep 135 deg (lower left) clockwise through 270 (top) to 405 (lower right).
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
    p.circle(8, 8, 8);
    const a = ((START + SWEEP * v) * Math.PI) / 180;
    p.line(8, 8, 8 + 7 * Math.cos(a), 8 + 7 * Math.sin(a));
  },
};
