// Spinner: eight dots on a ring; a three-dot head (2 px dots) runs clockwise over 1 px dots.
// The last frame flows into the first, so a looping player shows no jump.
const DOTS = [
  [4, 0, 0, 1],
  [7, 1, -1, 1],
  [8, 4, -1, 0],
  [7, 7, -1, -1],
  [4, 8, 0, -1],
  [1, 7, 1, -1],
  [0, 4, 1, 0],
  [1, 1, 1, 1],
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
    DOTS.forEach(([x, y]) => p.px(x, y));
    for (let k = 0; k < 3; k++) {
      const [x, y, dx, dy] = DOTS[(i - k + N) % N];
      p.px(x, y);
      p.px(x + dx, y);
      p.px(x, y + dy);
      p.px(x + dx, y + dy);
    }
  },
};
