import { BOX, wavePlot, sine } from '../wave/shape.mjs';
// A wave with a dotted ghost that wanders in phase and level.
export default {
  title: 'Drift',
  size: 'knob',
  tags: ['fx'],
  draw(p) {
    p.plot((t) => 0.7 * Math.sin(2 * Math.PI * (t + 0.18 + 0.1 * Math.sin(Math.PI * t * 2))), { ...BOX, dotted: true });
    wavePlot(p, sine);
  },
};
