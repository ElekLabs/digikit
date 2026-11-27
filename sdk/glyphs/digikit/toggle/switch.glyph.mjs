// Slide switch in the vertical middle of a knob tile: rounded track, 5x5 knob that slides right.
// Off = outline knob, on = filled knob.
const track = (p) => {
  p.line(2, 4, 14, 4);
  p.line(2, 12, 14, 12);
  p.line(0, 6, 0, 10);
  p.line(16, 6, 16, 10);
  p.px(1, 5);
  p.px(1, 11);
  p.px(15, 5);
  p.px(15, 11);
};
const knob = (p, x, filled) => (filled ? p.rect(x, 6, 5, 5) : p.frame(x, 6, 5, 5));

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Switch',
  size: 'knob',
  states: ['off', 'on'],
  tags: ['toggle'],
  invertible: false,
  draw(p, i) {
    track(p);
    knob(p, i === 0 ? 2 : 10, i === 1);
  },
  transition: {
    frames: 3,
    draw(p, from, to, t) {
      track(p);
      const a = from === 0 ? 2 : 10;
      const b = to === 0 ? 2 : 10;
      knob(p, Math.round(a + (b - a) * t), t >= 0.5 ? to === 1 : from === 1);
    },
  },
};
