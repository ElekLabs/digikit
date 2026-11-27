// Spectral tilt: a row of spectrum bars; the high bars rise with the value.
export default {
  title: 'Brightness',
  size: 'knob',
  range: 'unipolar',
  frames: 12,
  tags: ['synth', 'value'],
  draw(p, v) {
    p.line(1, 14, 15, 14);
    for (let i = 0; i < 5; i += 1) {
      const k = i / 4;
      const hh = Math.min(11, Math.max(2, Math.round(11 * ((1 - k) * (1 - 0.7 * v) + k * (0.15 + 0.85 * v)))));
      p.rect(2 + i * 3, 13 - hh, 2, hh);
    }
  },
};
