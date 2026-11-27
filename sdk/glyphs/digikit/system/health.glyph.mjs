// Health: severity grows with fill. ok is an outline circle with a check, warn a solid triangle with a
// cut-out !, error a solid disc with a cut-out X.
const put = (p, rows) => rows.forEach((r, y) => [...r].forEach((c, x) => c === "#" && p.px(x, y)));
const DISC = ['...###...', '.#######.', '.#######.', '#########', '#########', '#########', '.#######.', '.#######.', '...###...'];
const RING = ['...###...', '.##...##.', '.#.....#.', '#.......#', '#.......#', '#.......#', '.#.....#.', '.##...##.', '...###...'];
const TRI = ['....#....', '...###...', '...###...', '..#####..', '..#####..', '.#######.', '.#######.', '#########', '#########'];

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Health',
  size: 'icon',
  states: ['ok', 'warn', 'error'],
  tags: ['system'],
  draw(p, i) {
    if (i === 0) {
      put(p, RING);
      [[2, 4], [3, 5], [4, 4], [5, 3], [6, 2]].forEach(([x, y]) => p.px(x, y));
    } else if (i === 1) {
      put(p, TRI);
      for (const y of [3, 4, 5, 7]) p.px(4, y, 0);
    } else {
      put(p, DISC);
      for (let k = 0; k < 5; k++) {
        p.px(2 + k, 2 + k, 0);
        p.px(6 - k, 2 + k, 0);
      }
    }
  },
};
