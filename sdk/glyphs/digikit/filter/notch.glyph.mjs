import { baseline, poly } from './shape.mjs';
export default {
  title: 'Notch',
  size: 'knob',
  tags: ['filter'],
  draw(p) {
    baseline(p);
    poly(p, [[1, 6], [4, 6], [7, 12], [9, 12], [12, 6], [15, 6]]);
  },
};
