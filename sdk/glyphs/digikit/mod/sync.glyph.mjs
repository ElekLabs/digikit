// Tempo sync: two beamed eighth notes.
export default {
  title: 'Sync',
  size: 'knob',
  tags: ['mod'],
  draw(p) {
    for (const dx of [0, 7]) {
      p.line(4 + dx, 11, 6 + dx, 11);
      p.line(3 + dx, 12, 6 + dx, 12);
      p.line(3 + dx, 13, 5 + dx, 13);
      p.line(6 + dx, 3, 6 + dx, 10);
    }
    p.rect(6, 3, 8, 2);
  },
};
