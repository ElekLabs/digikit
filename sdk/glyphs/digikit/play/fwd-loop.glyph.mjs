import { drawMode } from './arrow.mjs';

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StaticGlyphDef} */
export default {
  title: 'Play fwd-loop',
  size: 'knob',
  tags: ['play'],
  draw(p) {
    drawMode(p, 2);
  },
};
