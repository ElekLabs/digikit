import { baseline, poly } from './shape.mjs';
export default {
  title: 'Peak',
  size: 'knob',
  tags: ['filter'],
  draw(p) {
    baseline(p);
    poly(p, [[1, 10], [4, 10], [7, 4], [9, 4], [12, 10], [15, 10]]);
  },
};
