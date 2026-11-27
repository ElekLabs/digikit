import { wavePlot } from '../wave/shape.mjs';
// Phase distortion: the sine's phase is bent so one edge sharpens toward a saw as the value rises.
export default {
  title: 'Phase distortion',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['synth', 'value'],
  draw(p, v) {
    const a = 0.5 - 0.42 * v;
    const warp = (u) => (u < a ? (u * 0.5) / a : 0.5 + ((u - a) * 0.5) / (1 - a));
    wavePlot(p, (t) => Math.sin(2 * Math.PI * (warp((t + 0.25) % 1) - 0.25)));
  },
};
