export default {
  title: 'Stop',
  size: 'icon',
  tags: ['transport'],
  draw(p) {
    p.rect(1, 1, 7, 7);
    for (const [x, y] of [[1, 1], [7, 1], [1, 7], [7, 7]]) p.px(x, y, 0);
  },
};
