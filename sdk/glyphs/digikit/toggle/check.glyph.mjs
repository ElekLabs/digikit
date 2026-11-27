// Checkbox: off = cut-corner outline, on = solid tile with a cut-out check mark. One transition frame: outline with the mark.
const FRAME = ['.#######.', '#.......#', '#.......#', '#.......#', '#.......#', '#.......#', '#.......#', '#.......#', '.#######.'];
const FILL = ['.#######.', '#########', '#########', '#########', '#########', '#########', '#########', '#########', '.#######.'];
const CHECK = [[1, 4], [2, 5], [3, 6], [4, 5], [5, 4], [6, 3], [7, 2]];
const put = (p, rows) => rows.forEach((r, y) => [...r].forEach((c, x) => c === '#' && p.px(x, y)));

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Check',
  size: 'icon',
  states: ['off', 'on'],
  tags: ['toggle'],
  draw(p, i) {
    put(p, i ? FILL : FRAME);
    if (i) CHECK.forEach(([x, y]) => p.px(x, y, 0));
  },
  transition: {
    frames: 1,
    draw(p) {
      put(p, FRAME);
      CHECK.forEach(([x, y]) => p.px(x, y));
    },
  },
};
