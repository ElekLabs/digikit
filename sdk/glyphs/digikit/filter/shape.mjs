// Shared low-pass drawing for the filter family (knob tiles, content x 1..15). Frequency runs left to right,
// level bottom to top; a dotted baseline marks silence. High pass is the mirror image.
export function baseline(p) {
  for (let x = 1; x <= 15; x += 2) p.px(x, 14);
}
export function lowPass(p) {
  baseline(p);
  p.line(1, 6, 8, 6);
  p.line(8, 6, 10, 4);
  p.line(10, 4, 11, 4);
  p.line(11, 4, 15, 12);
}

// Shared by the filter shapes beyond lp/hp/bp: a flat response line at `row`, with the dotted baseline.
export function poly(p, pts) {
  for (let i = 1; i < pts.length; i += 1) p.line(pts[i - 1][0], pts[i - 1][1], pts[i][0], pts[i][1]);
}
