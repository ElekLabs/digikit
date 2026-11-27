import { wavePlot, sine } from '../wave/shape.mjs';
// SOFT: sine times 1.6, clipped to the range (a soft square).
export default {
  title: 'SOFT',
  size: 'knob',
  tags: ['lfo'],
  draw: (p) => wavePlot(p, (t) => Math.max(-1, Math.min(1, 1.6 * sine(t)))),
};
