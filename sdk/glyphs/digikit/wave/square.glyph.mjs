import { BOX } from './shape.mjs';
// Same drawing as pulse at half width.
export default {
  title: 'Square',
  size: 'knob',
  tags: ['wave'],
  draw(p) {
    const { x0, y0, w, h } = BOX;
    const x1 = x0 + w - 1;
    const bot = y0 + h - 1;
    p.line(x0, y0, x0 + 7, y0);
    p.line(x0 + 7, y0, x0 + 7, bot);
    p.line(x0 + 7, bot, x1, bot);
  },
};
