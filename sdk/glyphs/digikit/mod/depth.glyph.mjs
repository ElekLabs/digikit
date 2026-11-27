import { wavePlot, sine } from '../wave/shape.mjs';
// Mod depth: a sine whose amplitude is the value, flipped for a negative depth.
export default {
  title: 'Depth',
  size: 'knob',
  range: 'bipolar',
  frames: 17,
  tags: ['mod', 'value'],
  draw: (p, v) => wavePlot(p, (t) => v * sine(t)),
};
