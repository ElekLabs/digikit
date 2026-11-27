// Module load state: an outline module tile (unloaded) fills from the centre into a solid tile (loaded).
const TILE = ['.##...##.', '#########', '#########', '#########', '#########', '#########', '#########', '#########', '.#######.'];
const on = (x, y) => TILE[y]?.[x] === '#';
// depth: distance to the outside by 4-neighbours; 1 on the rim
const depth = (x, y) => {
  if (!on(x, y)) return 0;
  let d = 1;
  while ([[1, 0], [-1, 0], [0, 1], [0, -1]].every(([dx, dy]) => on(x + dx * d, y + dy * d))) d++;
  return d;
};
const paint = (p, min) => {
  for (let y = 0; y < 9; y++)
    for (let x = 0; x < 9; x++) {
      const d = depth(x, y);
      if (d === 1 || (d > 0 && d >= min)) p.px(x, y);
    }
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Module load',
  size: 'icon',
  states: ['unloaded', 'loaded'],
  tags: ['system'],
  draw(p, i) {
    paint(p, i === 1 ? 1 : 99);
  },
  transition: {
    frames: 2,
    pairs: [
      ['unloaded', 'loaded'],
      ['loaded', 'unloaded'],
    ],
    draw(p, from, to, t) {
      const k = to === 1 ? t : 1 - t;
      paint(p, k < 0.5 ? 4 : 3);
    },
  },
};
