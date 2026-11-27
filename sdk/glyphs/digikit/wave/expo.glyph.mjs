import { BOX, wavePlot } from './shape.mjs';
// Exponential decay from the top to the bottom over the full width, then the jump back up (stock EXPO: unipolar).
export default {
  title: 'Expo',
  size: 'knob',
  tags: ['wave'],
  draw(p) {
    wavePlot(p, (t) => 2 * Math.exp(-4.5 * t) - 1, { ...BOX, period: BOX.w - 1 });
    p.line(BOX.x0 + BOX.w - 1, BOX.y0, BOX.x0 + BOX.w - 1, BOX.y0 + BOX.h - 1);
  },
};
