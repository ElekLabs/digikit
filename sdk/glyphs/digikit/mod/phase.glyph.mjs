import { BOX, wavePlot, sine } from '../wave/shape.mjs';
import { node } from '../env/node.mjs';
// Start phase: a sine with a hollow node on the curve at the start point.
export default {
  title: 'Phase',
  size: 'knob',
  tags: ['mod'],
  draw(p) {
    wavePlot(p, sine);
    const x = BOX.x0 + 3;
    node(p, x, BOX.y0 + Math.round(((1 - sine(3 / BOX.period)) / 2) * (BOX.h - 1)));
  },
};
