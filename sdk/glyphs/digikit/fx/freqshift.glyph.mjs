import { baseline } from '../filter/shape.mjs';
// Three spectrum lines slide with the shift; dotted lines mark their home positions.
const LINES = [[4, 7], [8, 3], [12, 9]]; // home column, top row
export default {
  title: 'Frequency shift',
  size: 'knob',
  range: 'bipolar',
  frames: 25,
  tags: ['fx', 'value'],
  invertible: true,
  draw(p, v) {
    baseline(p);
    const d = Math.round(v * 3);
    for (const [x, top] of LINES) {
      for (let y = top; y <= 13; y += 2) p.px(x, y);
      p.line(x + d, top, x + d, 13);
    }
  },
};
