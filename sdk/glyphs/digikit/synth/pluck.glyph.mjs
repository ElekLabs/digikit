import { BOX, wavePlot, sine } from '../wave/shape.mjs';
// A plucked string: a wave that dies away, its amplitude shrinking to nearly flat.
export default {
  title: 'Pluck',
  size: 'knob',
  tags: ['synth'],
  draw: (p) => wavePlot(p, (t) => sine(2 * t) * Math.max(0.08, 1 - 1.05 * t), BOX),
};
