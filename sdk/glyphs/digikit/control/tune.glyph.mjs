// Tune: the knob ring and pointer with semitone-style detent ticks one pixel in from the ring (every second
// pointer step, 13 in all) and a 3 px tick at 12 o'clock for 0.
const START = 135;
const SWEEP = 270;
const at = (r, deg) => {
  const a = (deg * Math.PI) / 180;
  return [8 + r * Math.cos(a), 8 + r * Math.sin(a)];
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Tune',
  size: 'knob',
  range: 'bipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    p.circle(8, 8, 8);
    for (let i = 0; i <= 12; i++) {
      const deg = START + (SWEEP * i) / 12;
      if (i === 6) p.line(8, 2, 8, 4);
      else p.px(...at(5.6, deg));
    }
    const a = ((START + (SWEEP * (v + 1)) / 2) * Math.PI) / 180;
    p.line(8, 8, 8 + 7 * Math.cos(a), 8 + 7 * Math.sin(a));
  },
};
