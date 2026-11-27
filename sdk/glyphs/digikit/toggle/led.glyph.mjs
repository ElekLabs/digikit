// LED: 5x5 dot centred in the 7x5 cell; off = outline, on = filled.
/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'LED',
  size: 'inline',
  states: ['off', 'on'],
  tags: ['toggle'],
  pixels: [
    `
..###..
.#...#.
.#...#.
.#...#.
..###..
`,
    `
..###..
.#####.
.#####.
.#####.
..###..
`,
  ],
};
