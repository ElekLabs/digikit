import { node } from './node.mjs';
export default {
  title: 'ADSR',
  size: 'knob',
  tags: ['env'],
  draw(p) {
    p.line(2, 14, 5, 5);
    p.line(5, 5, 8, 8);
    p.line(8, 8, 11, 8);
    p.line(11, 8, 14, 14);
    node(p, 5, 5);
    node(p, 8, 8);
  },
};
