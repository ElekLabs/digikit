// Shared drawing for the play-mode family. Knob-class tile 17x17, no ring.
// The arrow points right; reverse modes mirror the tile. Loop modes use a thin
// arrow on the top edge of a closed path that returns along the bottom.

/** Filled arrow head, tip at (tx, y), pointing right, half-height half. */
const head = (p, tx, y, half, left = false) => {
  for (let k = 0; k <= half; k++) p.line(left ? tx + k : tx - k, y - k, left ? tx + k : tx - k, y + k);
};

/** Pixels of the return path, clockwise from the top-right corner down, along the bottom, up the left side. */
const RETURN = [];
for (let y = 5; y <= 11; y++) RETURN.push([14, y]);
for (let x = 13; x >= 3; x--) RETURN.push([x, 12]);
for (let y = 11; y >= 5; y--) RETURN.push([2, y]);

/** Forward picture at step 0 (plain arrow) to 4 (loop arrow); 1 to 3 are the frames between. */
export const drawForward = (p, step = 0) => {
  if (step === 0) {
    p.rect(2, 7, 8, 3);
    head(p, 14, 8, 4);
    return;
  }
  const y = [8, 6, 5, 4, 4][step];
  const tip = [14, 14, 14, 13, 13][step];
  const lineEnd = [9, 14, 14, 12, 12][step];
  p.line(step === 4 ? 3 : 2, y, lineEnd, y);
  head(p, tip, y, 3);
  const n = [0, 0, 6, 15, RETURN.length][step];
  RETURN.slice(0, n).forEach(([x, yy]) => p.px(x, yy));
};

/** Mode tile for state i: 0 rev, 1 rev-loop, 2 fwd-loop, 3 fwd. */
export const drawMode = (p, i) => {
  const q = new p.constructor(p.w, p.h);
  drawForward(q, i === 1 || i === 2 ? 4 : 0);
  if (i < 2) q.flipX();
  p.blit(q, 0, 0);
};

/** Loop arrow without its head, for the turn between the two loop modes (frame 0..2, head faces left, bar, right). */
export const drawTurn = (p, toRight, frame) => {
  const q = new p.constructor(p.w, p.h);
  q.line(3, 4, 12, 4);
  RETURN.forEach(([x, y]) => q.px(x, y));
  const f = toRight ? frame : 2 - frame;
  if (f === 1) q.line(8, 1, 8, 7);
  else if (f === 0) head(q, 4, 4, 2, true);
  else head(q, 12, 4, 2);
  p.blit(q, 0, 0);
};
