import { BOX } from '../wave/shape.mjs';
// PULS: 25% pulse, high for the first quarter of the period (four of 16 columns).
export default {
  title: 'PULS',
  size: 'knob',
  tags: ['lfo'],
  draw(p) {
    const { x0, y0, w, h } = BOX;
    const bot = y0 + h - 1;
    p.line(x0, bot, x0, y0);
    p.line(x0, y0, x0 + 3, y0);
    p.line(x0 + 3, y0, x0 + 3, bot);
    p.line(x0 + 3, bot, x0 + w - 1, bot);
  },
};
