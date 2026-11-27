import { baseline } from './shape.mjs';
// A bank of narrow peaks of unequal height on a low floor.
export default {
  title: 'Resonator',
  size: 'knob',
  tags: ['filter'],
  draw(p) {
    baseline(p);
    p.line(1, 12, 15, 12);
    for (const [c, top] of [[3, 6], [8, 3], [13, 8]]) {
      p.line(c - 1, 12, c, top);
      p.line(c, top, c + 1, 12);
    }
  },
};
