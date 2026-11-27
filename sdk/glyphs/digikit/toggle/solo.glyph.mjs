// Solo: headphones. Off = outline ear cups, on = solid ear cups.
const BAND = ['..#####..', '.#.....#.', '#.......#', '#.......#'];
const put = (p, rows, y0 = 0) => rows.forEach((r, y) => [...r].forEach((c, x) => c === '#' && p.px(x, y + y0)));
const OFF = ['###...###', '#.#...#.#', '#.#...#.#', '#.#...#.#', '###...###'];
const ON = ['###...###', '###...###', '###...###', '###...###', '###...###'];

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Solo',
  size: 'icon',
  states: ['off', 'on'],
  tags: ['toggle'],
  draw(p, i) {
    put(p, BAND);
    put(p, i ? ON : OFF, 4);
  },
};
