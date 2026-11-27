// Two-state switch: a rounded track with a knob at the left (off) or right (on), three frames of slide in between.
const track = (p) => {
  p.line(2, 0, 10, 0);
  p.line(2, 6, 10, 6);
  p.px(1, 1);
  p.px(0, 2);
  p.px(0, 3);
  p.px(0, 4);
  p.px(1, 5);
  p.px(11, 1);
  p.px(12, 2);
  p.px(12, 3);
  p.px(12, 4);
  p.px(11, 5);
};

/** Knob 3 px wide whose left edge is at x. */
const knob = (p, x, filled) => {
  if (filled) p.rect(x, 2, 3, 3);
  else p.frame(x, 2, 3, 3);
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Switch',
  size: [13, 7],
  states: ['off', 'on'],
  tags: ['control', 'toggle'],
  invertible: false,
  draw(p, i) {
    track(p);
    knob(p, i === 0 ? 2 : 8, i === 1);
  },
  transition: {
    frames: 3,
    draw(p, from, to, t) {
      track(p);
      const a = from === 0 ? 2 : 8;
      const b = to === 0 ? 2 : 8;
      knob(p, Math.round(a + (b - a) * t), t >= 0.5 ? to === 1 : from === 1);
    },
  },
};
