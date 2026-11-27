import { drawForward, drawMode, drawTurn } from './arrow.mjs';

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').StatesGlyphDef} */
export default {
  title: 'Play mode',
  size: 'knob',
  states: ['rev', 'rev-loop', 'fwd-loop', 'fwd'],
  tags: ['play'],
  draw: drawMode,
  transition: {
    frames: 3,
    draw(p, from, to, t) {
      const a = Math.min(from, to);
      const k = Math.round(t * 4); // 1..3
      const forward = to > from;
      if (a === 1) {
        // rev-loop <-> fwd-loop: the head squeezes to a bar and opens the other way.
        drawTurn(p, forward, k - 1);
      } else {
        // plain <-> loop, drawn forward-facing and mirrored for the reverse pair.
        const loopFirst = (a === 0 && !forward) || (a === 2 && forward);
        const f = loopFirst ? 4 - k : k;
        const q = new p.constructor(p.w, p.h);
        drawForward(q, f);
        if (a === 0) q.flipX();
        p.blit(q, 0, 0);
      }
    },
  },
};
