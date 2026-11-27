// Shared box and helpers for the wave family (knob tiles, 1 px empty ring). Not a glyph.
// 15 columns, 9 rows (centre row 8). A period is 16 samples so triangle slopes are exact 45 degree lines.
export const BOX = { x0: 1, y0: 4, w: 15, h: 9, period: 16 };
export function wavePlot(p, fn, box = BOX) {
  p.plot(fn, box);
}
export const sine = (t) => Math.sin(2 * Math.PI * t);
export const tri = (t) => (t < 0.25 ? 4 * t : t < 0.75 ? 2 - 4 * t : 4 * t - 4);
// Seeded generator so noise is the same in every build.
export function seeded(seed) {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}
// Plot by column: fn(i) gives the level (-1..1) of column i (0 = leftmost column of the box).
export function colPlot(p, fn, box = BOX) {
  p.plot((t) => fn(Math.round(t * box.w)), { ...box, period: box.w });
}
