// Speaker: on = solid speaker with two waves; muted = outline speaker with a cross.
const speaker = (p, filled) => {
  if (filled) {
    p.line(1, 3, 1, 5);
    p.line(2, 2, 2, 6);
    p.line(3, 1, 3, 7);
  } else {
    p.line(1, 3, 1, 5);
    p.px(2, 2);
    p.px(2, 6);
    p.line(3, 1, 3, 7);
  }
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Mute',
  size: 'icon',
  states: ['on', 'muted'],
  tags: ['toggle'],
  draw(p, i) {
    speaker(p, i === 0);
    if (i === 0) {
      p.line(5, 3, 5, 5);
      p.line(7, 2, 7, 6);
    } else {
      p.line(5, 3, 7, 5);
      p.line(5, 5, 7, 3);
    }
  },
};
