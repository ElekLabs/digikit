import { node } from './node.mjs';
// Attack-release: rise to a held level, then release; two nodes.
export default {
  title: 'AR',
  size: 'knob',
  tags: ['env'],
  draw(p) {
    p.line(1, 14, 4, 5);
    p.line(4, 5, 11, 5);
    p.line(11, 5, 15, 14);
    node(p, 4, 5);
    node(p, 11, 5);
  },
};
