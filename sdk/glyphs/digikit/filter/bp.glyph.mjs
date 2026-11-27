import { baseline } from './shape.mjs';
export default {
  title: 'Band pass',
  size: 'knob',
  tags: ['filter'],
  draw(p) {
    baseline(p);
    p.line(1, 12, 3, 12);
    p.line(3, 12, 7, 4);
    p.line(7, 4, 9, 4);
    p.line(9, 4, 13, 12);
    p.line(13, 12, 15, 12);
    p.px(8, 3);
  },
};
