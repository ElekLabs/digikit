import { BOX } from '../wave/shape.mjs';
// Cycles across the tile rise from 1 to 3 with the value.
export default {
  title: 'Rate',
  size: 'knob',
  range: 'unipolar',
  frames: 9,
  tags: ['fx', 'value'],
  draw(p, v) {
    const cycles = 1 + 2 * v;
    p.plot((t) => Math.sin(2 * Math.PI * t), { ...BOX, period: BOX.w / cycles });
  },
};
