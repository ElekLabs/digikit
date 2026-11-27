// 3x3 hollow envelope node centred on (x, y). Not a glyph.
export function node(p, x, y) {
  p.frame(x - 1, y - 1, 3, 3);
  p.px(x, y, 0);
}
