import { node } from './node.mjs';
export default {
  title: 'AD',
  size: 'knob',
  tags: ['env'],
  draw(p) {
    p.line(2, 14, 5, 5);
    p.line(5, 5, 14, 14);
    node(p, 5, 5);
  },
};
