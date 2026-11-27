import { baseline, poly } from './shape.mjs';
// The low-pass curve with its resonant peak rising from none to tall.
export default {
  title: 'Resonance',
  size: 'knob',
  range: 'unipolar',
  frames: 13,
  tags: ['filter', 'value'],
  draw(p, v) {
    baseline(p);
    const top = 8 - Math.round(v * 6);
    poly(p, [[1, 8], [7, 8], [9, top], [10, top], [15, 13]]);
  },
};
