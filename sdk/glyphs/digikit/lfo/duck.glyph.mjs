import { BOX, wavePlot } from '../wave/shape.mjs';
// DUCK: 1 - 2 exp(-5 t), a fast rise that settles at the top, then the drop (sidechain duck).
export default {
  title: 'DUCK',
  size: 'knob',
  tags: ['lfo'],
  draw(p) {
    wavePlot(p, (t) => 1 - 2 * Math.exp(-5 * t), { ...BOX, period: BOX.w - 1 });
    p.line(BOX.x0 + BOX.w - 1, BOX.y0, BOX.x0 + BOX.w - 1, BOX.y0 + BOX.h - 1);
  },
};
