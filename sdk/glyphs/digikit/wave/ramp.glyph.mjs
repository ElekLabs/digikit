import { BOX, colPlot } from './shape.mjs';
// Ramp up at 45 degrees from the bottom to the top, then hold (the ramp that does not repeat, unipolar).
export default {
  title: 'Ramp',
  size: 'knob',
  tags: ['wave'],
  draw: (p) => colPlot(p, (i) => Math.min(1, -1 + i / 4)),
};
