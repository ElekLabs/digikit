// Decay curve shape: log (-1, slow start then drop) through linear (0) to exp (+1, fast drop then tail).
export default {
  title: 'Curve',
  size: 'knob',
  range: 'bipolar',
  frames: 13,
  tags: ['env', 'value'],
  draw(p, v) {
    const k = 3.2 * Math.abs(v);
    const f = (t) => (k < 0.05 ? t : (1 - Math.exp(-k * t)) / (1 - Math.exp(-k)));
    let py;
    for (let x = 3; x <= 13; x += 1) {
      const t = (x - 3) / 10;
      const y = 3 + Math.round((v >= 0 ? f(t) : 1 - f(1 - t)) * 10);
      if (py !== undefined) p.line(x - 1, py, x, y);
      else p.px(x, y);
      py = y;
    }
  },
};
