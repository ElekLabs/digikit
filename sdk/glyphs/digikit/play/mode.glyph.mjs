import { drawMode, drawStep } from './arrow.mjs';

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Play mode',
  size: 'knob',
  states: ['rev', 'rev-loop', 'fwd-loop', 'fwd'],
  tags: ['play'],
  draw: drawMode,
  transition: {
    frames: 3,
    pairs: [
      ['rev', 'rev-loop'],
      ['rev-loop', 'rev'],
      ['rev-loop', 'fwd-loop'],
      ['fwd-loop', 'rev-loop'],
      ['fwd-loop', 'fwd'],
      ['fwd', 'fwd-loop'],
    ],
    draw: drawStep,
  },
};
