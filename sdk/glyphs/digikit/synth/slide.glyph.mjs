// Slide: two step levels joined by a slanted glide line.
export default {
  title: 'Slide',
  size: 'knob',
  tags: ['synth'],
  draw(p) {
    p.rect(1, 11, 5, 2);
    p.rect(11, 4, 5, 2);
    p.line(5, 11, 11, 5);
    p.line(5, 12, 11, 6);
  },
};
