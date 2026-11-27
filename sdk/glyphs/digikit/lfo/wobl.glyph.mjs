import { wavePlot, sine } from '../wave/shape.mjs';
// WOBL: a sine with a dent in each hump (the module's shape is a sine plus a third harmonic; the dent is exaggerated to read at 1x).
export default {
  title: 'WOBL',
  size: 'knob',
  tags: ['lfo'],
  draw: (p) => wavePlot(p, (t) => (sine(t) + 0.55 * sine(3 * t)) / 1.0),
};
