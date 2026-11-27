// An accented step: an accent mark over a tall step among small ones.
export default {
  title: 'Accent',
  size: 'knob',
  tags: ['synth'],
  draw(p) {
    p.line(4, 2, 9, 5);
    p.line(4, 8, 9, 5);
    p.line(4, 2, 4, 2);
    p.rect(2, 12, 3, 2);
    p.rect(7, 9, 3, 5);
    p.rect(12, 12, 3, 2);
  },
};
