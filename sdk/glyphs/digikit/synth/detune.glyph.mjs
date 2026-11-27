import { BOX, sine } from '../wave/shape.mjs';
// Two sines drifting apart in phase: solid is the first oscillator, dotted the detuned second.
export default {
  title: 'Detune',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['synth', 'value'],
  draw(p, v) {
    p.plot(sine, BOX);
    p.plot((t) => sine(t - 0.25 * v), { ...BOX, dotted: true });
  },
};
