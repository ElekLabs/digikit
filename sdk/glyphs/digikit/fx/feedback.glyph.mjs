// A signal line with a return path that fills in (dotted when empty, solid when full).
const path = [];
for (let y = 10; y >= 4; y -= 1) path.push([13, y]);
for (let x = 12; x >= 3; x -= 1) path.push([x, 4]);
for (let y = 5; y <= 9; y += 1) path.push([3, y]);
export default {
  title: 'Feedback',
  size: 'knob',
  range: 'unipolar',
  frames: 13,
  tags: ['fx', 'value'],
  draw(p, v) {
    p.line(1, 11, 15, 11);
    const n = Math.round(v * path.length);
    path.forEach(([x, y], i) => {
      if (i < n || i % 2 === 0) p.px(x, y);
    });
    p.px(2, 8);
    p.px(4, 8);
    p.px(3, 9);
  },
};
