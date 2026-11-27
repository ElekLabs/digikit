import { wavePlot, sine } from '../wave/shape.mjs';
// ABSN: rectified sine, two humps per period, drawn on the full height of the box with a cusp between them.
export default {
  title: 'ABSN',
  size: 'knob',
  tags: ['lfo'],
  draw: (p) => wavePlot(p, (t) => 2 * Math.abs(sine(t)) - 1),
};
