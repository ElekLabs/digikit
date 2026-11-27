import { BOX, wavePlot, seeded } from './shape.mjs';
const rnd = seeded(11);
const ys = Array.from({ length: BOX.w }, () => rnd() * 2 - 1);
export default {
  title: 'Noise',
  size: 'knob',
  tags: ['wave'],
  draw: (p) => wavePlot(p, (t) => ys[Math.round(t * BOX.period)]),
};
