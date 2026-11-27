import { BOX } from './shape.mjs';
// Pulse width from 3 to 13 of 15 columns; the high part starts at the left edge.
export default {
  title: 'Pulse',
  size: 'knob',
  range: 'unipolar',
  frames: 11,
  tags: ['wave', 'value'],
  draw(p, v) {
    const hi = 3 + Math.round(v * 10);
    const top = BOX.y0;
    const bot = BOX.y0 + BOX.h - 1;
    const x0 = BOX.x0;
    const x1 = x0 + BOX.w - 1;
    p.line(x0, bot, x0, top);
    p.line(x0, top, x0 + hi - 1, top);
    p.line(x0 + hi - 1, top, x0 + hi - 1, bot);
    p.line(x0 + hi - 1, bot, x1, bot);
  },
};
