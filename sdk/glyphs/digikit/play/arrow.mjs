// Shared drawing for the play-mode family. Knob-class tile 17x17, 1 px empty margin, no ring.
// fwd: bold right arrow in the upper part of the tile. fwd-loop adds a 1 px return arc below it (a U-turn from
// under the head back to under the tail and up into it). rev and rev-loop mirror the tile.

/** Pixels of the return arc in drawing order: down under the head, along the bottom, up to the tail. */
const ARC = [];
for (let y = 9; y <= 12; y++) ARC.push([13, y]);
for (let x = 12; x >= 3; x--) ARC.push([x, 13]);
for (let y = 12; y >= 8; y--) ARC.push([2, y]);
ARC.push([1, 9], [3, 9]);

/** Bold arrow: shaft x 1..11 (3 rows), solid head with tip at x 15. */
const arrow = (p, squeeze = 0) => {
  if (squeeze) {
    p.rect(6, 4, 5, 3);
    p.line(8, 1, 8, 9);
    return;
  }
  p.rect(1, 4, 11, 3);
  for (let k = 0; k <= 4; k++) p.line(15 - k, 5 - k, 15 - k, 5 + k);
};

/** Forward picture with the first `n` pixels of the return arc. */
export const drawForward = (p, n = 0) => {
  arrow(p);
  ARC.slice(0, n).forEach(([x, y]) => p.px(x, y));
};

const mirrored = (p, draw) => {
  const q = new p.constructor(p.w, p.h);
  draw(q);
  q.flipX();
  p.blit(q, 0, 0);
};

/** Mode tile for state i: 0 rev, 1 rev-loop, 2 fwd-loop, 3 fwd. */
export const drawMode = (p, i) => {
  const loop = i === 1 || i === 2 ? ARC.length : 0;
  if (i < 2) mirrored(p, (q) => drawForward(q, loop));
  else drawForward(p, loop);
};

/** Step between neighbours: the arc grows or shrinks (rev, rev-loop and fwd-loop, fwd), or the arrow flips. */
export const drawStep = (p, from, to, t) => {
  const lo = Math.min(from, to);
  if (lo === 1) {
    // rev-loop <-> fwd-loop: half arc, arrow squeezed to its mid line, half arc of the other side.
    const f = Math.round(t * 4); // 1..3
    const half = Math.round(ARC.length / 2);
    const fromLeft = from < to; // coming from rev-loop (mirrored)
    const first = f < 2 ? fromLeft : !fromLeft;
    const draw = (q) => (f === 2 ? (arrow(q, 1), 0) : drawForward(q, half));
    if (first) mirrored(p, draw);
    else draw(p);
    return;
  }
  // The arc is present at rev-loop (lo 0, mirrored) and at fwd-loop (lo 2); it grows towards the loop state.
  const toLoop = lo === 0 ? from === 0 : from === 3;
  const n = Math.round((toLoop ? t : 1 - t) * ARC.length);
  if (lo === 0) mirrored(p, (q) => drawForward(q, n));
  else drawForward(p, n);
};
