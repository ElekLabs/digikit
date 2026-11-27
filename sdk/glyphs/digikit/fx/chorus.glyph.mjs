import { BOX, wavePlot, sine } from '../wave/shape.mjs';
// A solid wave with one dotted copy offset in phase.
export default {
  title: 'Chorus',
  size: 'knob',
  tags: ['fx'],
  draw(p) {
    p.plot((t) => Math.sin(2 * Math.PI * (t + 0.2)), { ...BOX, dotted: true });
    wavePlot(p, sine);
  },
};
