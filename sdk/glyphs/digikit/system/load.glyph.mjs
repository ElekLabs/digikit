// Module load state: an empty dotted slot (unloaded) fills up into a solid module (loaded).
const box = (p, on = 1) => {
  p.line(2, 1, 6, 1, on);
  p.line(2, 7, 6, 7, on);
  p.line(1, 2, 1, 6, on);
  p.line(7, 2, 7, 6, on);
};

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Module load',
  size: 'icon',
  states: ['unloaded', 'loaded'],
  tags: ['system'],
  draw(p, i) {
    if (i === 1) {
      p.rect(1, 2, 7, 5);
      p.rect(2, 1, 5, 7);
      return;
    }
    // dotted outline
    for (let x = 2; x <= 6; x += 2) {
      p.px(x, 1);
      p.px(x, 7);
    }
    for (let y = 2; y <= 6; y += 2) {
      p.px(1, y);
      p.px(7, y);
    }
  },
  transition: {
    frames: 3,
    draw(p, from, to, t) {
      const k = Math.round((from === 0 ? t : 1 - t) * 4); // 1..3: solid outline, then fill rising
      box(p);
      if (k >= 2) p.rect(2, 7 - (k - 1) * 2 + 1, 5, (k - 1) * 2 - 1 > 0 ? 6 - (7 - (k - 1) * 2) : 0);
    },
  },
};
