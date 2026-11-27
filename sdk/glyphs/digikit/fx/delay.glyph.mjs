import { baseline } from '../filter/shape.mjs';
// A pulse followed by evenly spaced echoes that get shorter.
export default {
  title: 'Delay',
  size: 'knob',
  tags: ['fx'],
  draw(p) {
    baseline(p);
    p.rect(2, 3, 2, 11);
    for (const [x, h] of [[7, 8], [11, 5], [15, 3]]) p.line(x, 13, x, 13 - h + 1);
  },
};
