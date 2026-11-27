// Progress: a ring of 24 pixels; the finished part is solid, the rest dotted, and the centre fills at 100%.
const RING = ['...###...', '.##...##.', '.#.....#.', '#.......#', '#.......#', '#.......#', '.#.....#.', '.##...##.', '...###...'];
const path = [];
{
  // clockwise from the top centre
  const on = (x, y) => RING[y]?.[x] === '#';
  const seen = new Set();
  let [x, y] = [4, 0];
  for (;;) {
    path.push([x, y]);
    seen.add(`${x},${y}`);
    const near = [[1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1], [1, -1]]
      .map(([dx, dy]) => [x + dx, y + dy])
      .filter(([a, b]) => on(a, b) && !seen.has(`${a},${b}`));
    if (!near.length) break;
    // prefer axis-aligned neighbours so the walk stays on the ring
    near.sort((a, b) => Math.abs(a[0] - x) + Math.abs(a[1] - y) - (Math.abs(b[0] - x) + Math.abs(b[1] - y)));
    [x, y] = near[0];
  }
}
const N = path.length;

/** @type {import('../../../../packages/dkglyph/src/glyph.ts').ValueGlyphDef} */
export default {
  title: 'Progress',
  size: 'icon',
  range: 'unipolar',
  frames: 13,
  tags: ['system', 'value'],
  draw(p, v) {
    const k = Math.round(v * N);
    path.forEach(([x, y], i) => {
      if (i < k || i % 2 === 0) p.px(x, y);
    });
    if (k >= N) p.rect(3, 3, 3, 3);
  },
};
