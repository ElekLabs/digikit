import { node } from './node.mjs';
// Decay length: the falling segment from the peak node reaches the base later as the value rises.
export default {
  title: 'Decay',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['env', 'value'],
  draw(p, v) {
    const x = 7 + Math.round(v * 8);
    p.line(1, 14, 4, 5);
    p.line(4, 5, x, 14);
    if (x < 15) p.line(x, 14, 15, 14);
    node(p, 4, 5);
  },
};
