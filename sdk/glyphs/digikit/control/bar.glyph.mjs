// Bar: cut-corner frame with a filled amount bar growing left to right, dotted scale above and below
// (the fader turned on its side).
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Bar',
  size: 'knob',
  range: 'unipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    for (const x of [1, 5, 9, 13]) {
      p.line(x, 1, x, 2);
      p.line(x, 14, x, 15);
    }
    p.line(2, 4, 14, 4);
    p.line(2, 12, 14, 12);
    p.line(1, 5, 1, 11);
    p.line(15, 5, 15, 11);
    const w = Math.round(v * 11);
    if (w > 0) p.rect(3, 6, w, 5);
  },
};
