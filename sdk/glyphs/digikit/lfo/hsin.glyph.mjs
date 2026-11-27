import { wavePlot, sine } from '../wave/shape.mjs';
// HSIN: positive half of a sine, then zero.
export default {
  title: 'HSIN',
  size: 'knob',
  tags: ['lfo'],
  draw: (p) => wavePlot(p, (t) => (t < 0.5 ? sine(t) : 0)),
};
