import { baseline } from '../filter/shape.mjs';
// Two bars move apart from the centre toward L and R.
export default {
  title: 'Spread',
  size: 'knob',
  range: 'unipolar',
  frames: 11,
  tags: ['fx', 'value'],
  draw(p, v) {
    baseline(p);
    for (let y = 3; y <= 12; y += 2) p.px(8, y);
    const g = Math.round(v * 5);
    p.rect(6 - g, 3, 2, 10);
    p.rect(9 + g, 3, 2, 10);
  },
};
