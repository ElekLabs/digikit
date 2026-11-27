import { wavePlot } from '../wave/shape.mjs';
// STP4: staircase up in four levels, four columns each.
export default {
  title: 'STP4',
  size: 'knob',
  tags: ['lfo'],
  draw: (p) => wavePlot(p, (t) => (2 * Math.floor(Math.floor(t * 32) / 8)) / 3 - 1),
};
