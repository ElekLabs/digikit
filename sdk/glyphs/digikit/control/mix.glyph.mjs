// Mix: one sine across the tile; the dry part is dotted, the wet part solid. The boundary moves left to right.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Mix',
  size: 'knob',
  range: 'unipolar',
  frames: 25,
  tags: ['control', 'value'],
  invertible: false,
  draw(p, v) {
    const box = { x0: 1, y0: 4, w: 15, h: 9, period: 16 };
    const wet = new p.constructor(p.w, p.h);
    const dry = new p.constructor(p.w, p.h);
    const sine = (t) => Math.sin(2 * Math.PI * t);
    wet.plot(sine, box);
    dry.plot(sine, { ...box, dotted: true });
    const cut = 1 + Math.round(v * 15);
    for (let y = 0; y < p.h; y++)
      for (let x = 0; x < p.w; x++) if ((x >= cut ? wet : dry).get(x, y)) p.px(x, y);
  },
};
