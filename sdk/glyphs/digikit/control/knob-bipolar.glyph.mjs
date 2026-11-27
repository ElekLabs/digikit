// Bipolar knob: ring and pointer as the knob, dotted tick at 12 o'clock, - and + beside the bottom gap.
const START = 135;
const SWEEP = 270;

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Knob bipolar',
  size: 'knob',
  range: 'bipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    p.circle(8, 8, 8);
    for (const y of [1, 3]) p.px(8, y);
    const a = ((START + SWEEP * (v + 1) / 2) * Math.PI) / 180;
    p.line(8, 8, 8 + 7 * Math.cos(a), 8 + 7 * Math.sin(a));
    p.line(0, 15, 2, 15);
    p.line(14, 15, 16, 15);
    p.px(15, 14);
    p.px(15, 16);
  },
};
