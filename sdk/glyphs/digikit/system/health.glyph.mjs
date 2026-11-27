// Health: severity grows with fill. ok is an outline ring with a check, warn a solid triangle with a
// cut-out bar, error a solid disc with a cut-out cross.
const ring = (p) => {
  p.line(3, 1, 5, 1);
  p.line(3, 7, 5, 7);
  p.line(1, 3, 1, 5);
  p.line(7, 3, 7, 5);
  p.px(2, 2);
  p.px(6, 2);
  p.px(2, 6);
  p.px(6, 6);
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Health',
  size: 'icon',
  states: ['ok', 'warn', 'error'],
  tags: ['system'],
  draw(p, i) {
    if (i === 0) {
      ring(p);
      p.px(3, 4);
      p.px(4, 5);
      p.px(5, 4);
      p.px(6, 3);
    } else if (i === 1) {
      const widths = [1, 3, 3, 5, 5, 7, 7];
      widths.forEach((w, k) => p.line(4 - (w - 1) / 2, k + 1, 4 + (w - 1) / 2, k + 1));
      p.px(4, 3, 0);
      p.px(4, 4, 0);
      p.px(4, 6, 0);
    } else {
      p.rect(1, 3, 7, 3);
      p.rect(3, 1, 3, 7);
      p.rect(2, 2, 5, 5);
      for (let k = 0; k < 3; k++) {
        p.px(3 + k, 3 + k, 0);
        p.px(5 - k, 3 + k, 0);
      }
    }
  },
};
