// Pitch bend: a line that starts flat and bends up (positive) or down (negative); dotted centre line.
export default {
  title: 'Bend',
  size: 'knob',
  range: 'bipolar',
  frames: 13,
  tags: ['synth', 'value'],
  draw(p, v) {
    for (let x = 1; x <= 15; x += 2) p.px(x, 8);
    const d = Math.round(v * 6);
    p.line(1, 8, 5, 8);
    if (d === 0) p.line(5, 8, 15, 8);
    else {
      p.line(5, 8, 12, 8 - d);
      p.line(12, 8 - d, 15, 8 - d);
    }
  },
};
