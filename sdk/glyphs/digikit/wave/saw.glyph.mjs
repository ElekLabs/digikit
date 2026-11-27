import { BOX, wavePlot } from './shape.mjs';
// Ramp over the full width, then the drop.
export default {
  title: 'Saw',
  size: 'knob',
  tags: ['wave'],
  draw(p) {
    wavePlot(p, (t) => 2 * t - 1, { ...BOX, period: BOX.w - 1 });
    p.line(BOX.x0 + BOX.w - 1, BOX.y0, BOX.x0 + BOX.w - 1, BOX.y0 + BOX.h - 1);
  },
};
