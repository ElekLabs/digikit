// Steps: eight detent dots on the 270 degree knob arc; the selected detent is a 2 px mark and a pointer line
// from the centre points at it.
const N = 8;
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Steps',
  size: 'knob',
  range: 'unipolar',
  frames: N,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    const sel = Math.round(v * (N - 1));
    for (let i = 0; i < N; i++) {
      const a = ((135 + (270 * i) / (N - 1)) * Math.PI) / 180;
      const c = Math.cos(a);
      const s = Math.sin(a);
      p.px(8 + 7 * c, 8 + 7 * s);
      if (i === sel) {
        p.px(8 + 6 * c, 8 + 6 * s);
        p.line(8, 8, 8 + 4 * c, 8 + 4 * s);
      }
    }
    p.px(8, 8);
  },
};
