import { wavePlot, sine } from '../wave/shape.mjs';
// FM index: a sine whose phase is modulated; more sidebands (wiggle) as the index rises.
export default {
  title: 'FM index',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['synth', 'value'],
  draw: (p, v) => wavePlot(p, (t) => sine(t + v * 0.22 * sine(3 * t))),
};
