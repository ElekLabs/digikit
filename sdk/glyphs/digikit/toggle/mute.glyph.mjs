// Speaker: on = solid speaker with two waves; muted = outline speaker with a cross.
const put = (p, rows) => rows.forEach((r, y) => [...r].forEach((c, x) => c === "#" && p.px(x, y)));
const SOLID = ['....#....', '...##....', '..###....', '#####....', '#####....', '#####....', '..###....', '...##....', '....#....'];
const OUTLINE = ['....#....', '...##....', '..#.#....', '###.#....', '#...#....', '###.#....', '..#.#....', '...##....', '....#....'];

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Mute',
  size: 'icon',
  states: ['on', 'muted'],
  tags: ['toggle'],
  draw(p, i) {
    if (i === 0) {
      put(p, SOLID);
      p.line(6, 3, 6, 5);
      p.line(8, 1, 8, 7);
    } else {
      put(p, OUTLINE);
      for (let k = 0; k < 4; k++) {
        p.px(5 + k, 2 + k + 0);
        p.px(8 - k, 2 + k + 0);
      }
    }
  },
};
