// Sample slot: a box with a decaying waveform burst.
const cols = { 3: [4, 4], 4: [2, 6], 5: [3, 5] };

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StaticGlyphDef} */
export default {
  title: 'Sample slot',
  size: 'icon',
  tags: ['play'],
  draw(p) {
    p.line(2, 1, 6, 1);
    p.line(2, 7, 6, 7);
    p.line(1, 2, 1, 6);
    p.line(7, 2, 7, 6);
    for (const [x, [a, b]] of Object.entries(cols)) p.line(Number(x), a, Number(x), b);
  },
};
