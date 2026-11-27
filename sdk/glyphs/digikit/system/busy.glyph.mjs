// Spinner: eight dots on a ring, a four-dot comet runs clockwise. Frame i lights dots i to i-3;
// the last frame flows into the first, so a looping player shows no jump.
const DOTS = [
  [4, 1],
  [6, 2],
  [7, 4],
  [6, 6],
  [4, 7],
  [2, 6],
  [1, 4],
  [2, 2],
];
const N = DOTS.length;

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Busy spinner',
  size: 'icon',
  range: 'unipolar',
  frames: N,
  tags: ['system', 'loop'],
  draw(p, v) {
    const i = Math.round(v * (N - 1));
    for (let k = 0; k < 4; k++) {
      const [x, y] = DOTS[(i - k + N) % N];
      p.px(x, y);
    }
  },
};
