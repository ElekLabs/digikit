import { baseline } from '../filter/shape.mjs';
// A pulse followed by a dense dotted tail that decays.
export default {
  title: 'Reverb',
  size: 'knob',
  tags: ['fx'],
  draw(p) {
    baseline(p);
    p.rect(2, 3, 2, 11);
    for (let x = 5; x <= 15; x += 1) {
      const h = Math.round(9 * Math.exp(-(x - 5) / 3.5));
      for (let y = 13; y > 13 - h; y -= 1) if ((x + y) % 2 === 0) p.px(x, y);
    }
  },
};
