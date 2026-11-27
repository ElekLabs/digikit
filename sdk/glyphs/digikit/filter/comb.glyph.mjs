import { baseline, poly } from './shape.mjs';
export default {
  title: 'Comb',
  size: 'knob',
  tags: ['filter'],
  draw(p) {
    baseline(p);
    poly(p, [[1, 11], [3, 6], [5, 11], [7, 6], [9, 11], [11, 6], [13, 11], [15, 6]]);
  },
};
