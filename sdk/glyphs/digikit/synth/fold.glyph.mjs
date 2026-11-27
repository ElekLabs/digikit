import { wavePlot, sine } from '../wave/shape.mjs';
// A sine driven harder into a triangle fold: the peaks turn back.
const fold = (x) => {
  const m = (((x + 1) % 4) + 4) % 4;
  return m < 2 ? m - 1 : 3 - m;
};
export default {
  title: 'Fold',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['synth', 'value'],
  draw: (p, v) => wavePlot(p, (t) => fold((1 + v * 1.6) * sine(t))),
};
