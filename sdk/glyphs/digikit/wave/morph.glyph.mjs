import { BOX, colPlot, sine, tri } from './shape.mjs';
// Sine, triangle, saw and square across the range: each column blends the two neighbouring shapes.
const W = BOX.w;
const shapes = [
  (i) => sine(i / BOX.period),
  (i) => tri(i / BOX.period),
  (i) => (2 * i) / (W - 1) - 1,
  (i) => (i < 8 ? 1 : -1),
];
export default {
  title: 'Morph',
  size: 'knob',
  range: 'unipolar',
  frames: 13,
  tags: ['wave', 'value'],
  draw(p, v) {
    const s = Math.min(2.999, v * 3);
    const k = Math.floor(s);
    const f = s - k;
    const a = shapes[k];
    const b = shapes[k + 1];
    colPlot(p, (i) => (1 - f) * a(i) + f * b(i));
  },
};
