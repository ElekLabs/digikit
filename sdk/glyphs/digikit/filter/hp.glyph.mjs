import { lowPass } from './shape.mjs';
export default {
  title: 'High pass',
  size: 'knob',
  tags: ['filter'],
  draw(p) {
    lowPass(p);
    p.flipX();
  },
};
