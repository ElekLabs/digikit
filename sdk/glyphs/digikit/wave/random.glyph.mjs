import { wavePlot, BOX } from './shape.mjs';
// Sample and hold: five held levels, three columns each.
const levels = [0.5, -0.5, 1, -1, 0];
export default {
  title: 'Random',
  size: 'knob',
  tags: ['wave'],
  draw: (p) => wavePlot(p, (t) => levels[Math.min(4, Math.floor((t * BOX.period) / 3))]),
};
