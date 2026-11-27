export interface PlotBox {
  x0: number;
  y0: number;
  w: number;
  h: number;
  /** When set, sample at t = i / period instead of i / (w - 1). */
  period?: number;
}

/** A 1-bit pixel grid with integer coordinates. Drawing outside is clipped and counted. */
export class Raster {
  readonly w: number;
  readonly h: number;
  readonly #bits: Uint8Array;
  #clipped = 0;

  constructor(w: number, h: number) {
    this.w = w;
    this.h = h;
    this.#bits = new Uint8Array(w * h);
  }

  /** Number of pixels that a draw call placed outside the raster. */
  get clipped(): number {
    return this.#clipped;
  }

  px(x: number, y: number, on: 0 | 1 | boolean = 1): this {
    const ix = Math.round(x);
    const iy = Math.round(y);
    if (ix < 0 || iy < 0 || ix >= this.w || iy >= this.h) {
      this.#clipped += 1;
      return this;
    }
    this.#bits[iy * this.w + ix] = on ? 1 : 0;
    return this;
  }

  get(x: number, y: number): 0 | 1 {
    if (x < 0 || y < 0 || x >= this.w || y >= this.h) return 0;
    return this.#bits[y * this.w + x] ? 1 : 0;
  }

  clear(): this {
    this.#bits.fill(0);
    return this;
  }

  /** Bresenham line, both ends included. */
  line(x0: number, y0: number, x1: number, y1: number, on: 0 | 1 = 1): this {
    let x = Math.round(x0);
    let y = Math.round(y0);
    const ex = Math.round(x1);
    const ey = Math.round(y1);
    const dx = Math.abs(ex - x);
    const dy = -Math.abs(ey - y);
    const sx = x < ex ? 1 : -1;
    const sy = y < ey ? 1 : -1;
    let err = dx + dy;
    for (;;) {
      this.px(x, y, on);
      if (x === ex && y === ey) break;
      const e2 = 2 * err;
      if (e2 >= dy) {
        err += dy;
        x += sx;
      }
      if (e2 <= dx) {
        err += dx;
        y += sy;
      }
    }
    return this;
  }

  /** Filled rectangle. */
  rect(x: number, y: number, w: number, h: number, on: 0 | 1 = 1): this {
    for (let j = 0; j < h; j += 1) for (let i = 0; i < w; i += 1) this.px(x + i, y + j, on);
    return this;
  }

  /** Rectangle outline, 1 pixel wide. */
  frame(x: number, y: number, w: number, h: number, on: 0 | 1 = 1): this {
    if (w < 1 || h < 1) return this;
    this.line(x, y, x + w - 1, y, on);
    this.line(x, y + h - 1, x + w - 1, y + h - 1, on);
    this.line(x, y, x, y + h - 1, on);
    this.line(x + w - 1, y, x + w - 1, y + h - 1, on);
    return this;
  }

  /** Midpoint circle, outline or filled. */
  circle(cx: number, cy: number, r: number, filled = false, on: 0 | 1 = 1): this {
    for (const [dx, dy] of circleOffsets(r)) {
      if (filled) this.line(cx - dx, cy + dy, cx + dx, cy + dy, on);
      else this.px(cx + dx, cy + dy, on);
    }
    return this;
  }

  /**
   * Arc of the midpoint circle from startDeg to endDeg. 0 degrees is right and angles grow clockwise on the screen.
   * The arc runs clockwise from start to end (end below start adds a turn).
   */
  arc(cx: number, cy: number, r: number, startDeg: number, endDeg: number, on: 0 | 1 = 1): this {
    const start = mod360(startDeg);
    let span = endDeg - startDeg;
    if (span < 0) span = mod360(span);
    if (span >= 360) span = 360;
    for (const [dx, dy] of circleOffsets(r)) {
      if (span < 360 && mod360(angleOf(dx, dy) - start) > span + 1e-9) continue;
      this.px(cx + dx, cy + dy, on);
    }
    return this;
  }

  /** Closed polygon outline. */
  poly(points: ReadonlyArray<readonly [number, number]>, on: 0 | 1 = 1): this {
    for (let i = 0; i < points.length; i += 1) {
      const a = points[i]!;
      const b = points[(i + 1) % points.length]!;
      this.line(a[0], a[1], b[0], b[1], on);
    }
    return this;
  }

  /**
   * Plot fn(t), t in [0,1], to [-1,1] (1 is the top row), one sample per column.
   * A column joins halfway to a neighbour only when that neighbour is 2 or more rows away, so the stroke never
   * breaks and 45 degree runs stay one pixel per column. With box.period, t = i / period.
   */
  plot(fn: (t: number) => number, box: PlotBox, on: 0 | 1 = 1): this {
    const { x0, y0, w, h, period } = box;
    const ys: number[] = [];
    for (let i = 0; i < w; i += 1) {
      const t = period !== undefined ? i / period : w === 1 ? 0 : i / (w - 1);
      const v = Math.max(-1, Math.min(1, fn(t)));
      ys.push(y0 + Math.round(((1 - v) / 2) * (h - 1)));
    }
    for (let i = 0; i < w; i += 1) {
      const y = ys[i]!;
      const reach = (n: number): number => (Math.abs(n - y) < 2 ? y : Math.round((n + y) / 2));
      const from = i === 0 ? y : reach(ys[i - 1]!);
      const to = i === w - 1 ? y : reach(ys[i + 1]!);
      for (let j = Math.min(from, y, to); j <= Math.max(from, y, to); j += 1) this.px(x0 + i, j, on);
    }
    return this;
  }

  invert(): this {
    for (let i = 0; i < this.#bits.length; i += 1) this.#bits[i] = this.#bits[i] ? 0 : 1;
    return this;
  }

  flipX(): this {
    for (let y = 0; y < this.h; y += 1)
      for (let x = 0; x < Math.floor(this.w / 2); x += 1) this.#swap(x, y, this.w - 1 - x, y);
    return this;
  }

  flipY(): this {
    for (let y = 0; y < Math.floor(this.h / 2); y += 1)
      for (let x = 0; x < this.w; x += 1) this.#swap(x, y, x, this.h - 1 - y);
    return this;
  }

  /** Copy the set pixels of other onto this raster at (x, y). */
  blit(other: Raster, x: number, y: number): this {
    for (let j = 0; j < other.h; j += 1)
      for (let i = 0; i < other.w; i += 1) if (other.get(i, j)) this.px(x + i, y + j, 1);
    return this;
  }

  equals(other: Raster): boolean {
    return this.w === other.w && this.h === other.h && this.#bits.every((b, i) => b === other.#bits[i]);
  }

  count(): number {
    return this.#bits.reduce((n, b) => n + b, 0);
  }

  toRows(): string[] {
    const rows: string[] = [];
    for (let y = 0; y < this.h; y += 1) {
      let row = '';
      for (let x = 0; x < this.w; x += 1) row += this.get(x, y) ? '#' : '.';
      rows.push(row);
    }
    return rows;
  }

  #swap(x0: number, y0: number, x1: number, y1: number): void {
    const a = y0 * this.w + x0;
    const b = y1 * this.w + x1;
    const t = this.#bits[a]!;
    this.#bits[a] = this.#bits[b]!;
    this.#bits[b] = t;
  }
}

const mod360 = (a: number): number => ((a % 360) + 360) % 360;
const angleOf = (dx: number, dy: number): number => mod360((Math.atan2(dy, dx) * 180) / Math.PI);

/** Offsets of the midpoint circle of radius r around the centre, each once. */
function circleOffsets(r: number): Array<readonly [number, number]> {
  const seen = new Map<string, readonly [number, number]>();
  const add = (x: number, y: number) => seen.set(`${x},${y}`, [x, y]);
  let x = Math.round(r);
  let y = 0;
  let err = 1 - x;
  while (x >= y) {
    for (const [a, b] of [
      [x, y],
      [y, x],
      [-y, x],
      [-x, y],
      [-x, -y],
      [-y, -x],
      [y, -x],
      [x, -y],
    ] as const)
      add(a, b);
    y += 1;
    if (err < 0) err += 2 * y + 1;
    else {
      x -= 1;
      err += 2 * (y - x) + 1;
    }
  }
  return [...seen.values()];
}
