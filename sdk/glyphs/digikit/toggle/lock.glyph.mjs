// Padlock: locked = filled body with keyhole and closed shackle; unlocked = outline body, right leg of the shackle open.
const shackle = (p, leg) => {
  p.line(3, 1, 5, 1);
  p.line(2, 2, 2, 3);
  if (leg >= 1) p.px(6, 2);
  if (leg >= 2) p.px(6, 3);
};
const body = (p, filled) => {
  if (filled) {
    p.rect(1, 4, 7, 4);
    p.px(4, 5, 0);
    p.px(4, 6, 0);
  } else p.frame(1, 4, 7, 4);
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Lock',
  size: 'icon',
  states: ['unlocked', 'locked'],
  tags: ['toggle'],
  draw(p, i) {
    shackle(p, i === 1 ? 2 : 0);
    body(p, i === 1);
  },
  transition: {
    frames: 2,
    draw(p, from, to, t) {
      const closing = to === 1;
      const leg = closing ? (t < 0.5 ? 0 : 1) : t < 0.5 ? 1 : 0;
      shackle(p, leg);
      body(p, t >= 0.5 ? to === 1 : from === 1);
    },
  },
};
