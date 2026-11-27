import { wavePlot, sine } from '../wave/shape.mjs';
// A sine driven into a soft clip: flat tops toward a square.
export default {
  title: 'Drive',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['synth', 'value'],
  draw(p, v) {
    const g = 1 + v * 7;
    wavePlot(p, (t) => Math.tanh(g * sine(t)) / Math.tanh(g));
  },
};
