import { sine } from '../wave/shape.mjs';
// A wave with a dotted wave one octave below (half the frequency).
export default {
  title: 'Sub oscillator',
  size: 'knob',
  tags: ['synth'],
  draw(p) {
    p.plot(sine, { x0: 1, y0: 2, w: 15, h: 5, period: 8 });
    p.plot(sine, { x0: 1, y0: 10, w: 15, h: 5, period: 16, dotted: true });
  },
};
