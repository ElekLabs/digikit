// Padlock: locked = solid body with keyhole and closed shackle; unlocked = outline body, right leg of the shackle lifted.
const put = (p, rows) => rows.forEach((r, y) => [...r].forEach((c, x) => c === "#" && p.px(x, y)));
const SHACKLE = [
  ['...###...', '..#...#..', '..#......', '..#......'],
  ['...###...', '..#...#..', '..#...#..', '..#......'],
  ['...###...', '..#...#..', '..#...#..', '..#...#..'],
];
const body = (p, filled) => {
  if (filled) {
    p.rect(0, 4, 9, 5);
    p.px(4, 5, 0);
    p.px(4, 6, 0);
  } else p.frame(0, 4, 9, 5);
};
const shackle = (p, leg) => put(p, SHACKLE[leg]);

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
    pairs: [
      ['unlocked', 'locked'],
      ['locked', 'unlocked'],
    ],
    draw(p, from, to, t) {
      const closing = to === 1;
      shackle(p, closing ? (t < 0.5 ? 1 : 2) : t < 0.5 ? 1 : 0);
      body(p, t >= 0.5 ? to === 1 : from === 1);
    },
  },
};
