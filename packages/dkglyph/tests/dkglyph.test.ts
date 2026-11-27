import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { checkSet, loadSet, Raster, renderGlyph } from '@digi/dkglyph';

/** Number of 8-connected groups of set pixels. */
function components(r: Raster): number {
  const seen = new Set<number>();
  let n = 0;
  for (let y = 0; y < r.h; y += 1)
    for (let x = 0; x < r.w; x += 1) {
      if (!r.get(x, y) || seen.has(y * r.w + x)) continue;
      n += 1;
      const stack = [[x, y]] as Array<[number, number]>;
      while (stack.length) {
        const [cx, cy] = stack.pop()!;
        if (cx < 0 || cy < 0 || cx >= r.w || cy >= r.h || !r.get(cx, cy) || seen.has(cy * r.w + cx)) continue;
        seen.add(cy * r.w + cx);
        for (let dy = -1; dy <= 1; dy += 1) for (let dx = -1; dx <= 1; dx += 1) stack.push([cx + dx, cy + dy]);
      }
    }
  return n;
}

describe('Raster', () => {
  it('draws arcs without gaps for every radius and sweep', () => {
    for (const r of [2, 3, 5, 6, 9, 13])
      for (const [a, b] of [
        [135, 405],
        [0, 90],
        [200, 340],
        [350, 20],
        [0, 360],
      ] as const) {
        const p = new Raster(2 * r + 1, 2 * r + 1);
        p.arc(r, r, r, a, b);
        expect(p.count(), `r=${r} ${a}..${b}`).toBeGreaterThan(0);
        expect(components(p), `r=${r} ${a}..${b}`).toBe(1);
      }
  });

  it('puts 0 degrees right and grows clockwise on the screen', () => {
    const p = new Raster(11, 11);
    p.arc(5, 5, 5, 0, 90);
    expect(p.get(10, 5)).toBe(1);
    expect(p.get(5, 10)).toBe(1);
    expect(p.get(5, 0)).toBe(0);
  });

  it('plots steep functions as one connected stroke with one pixel run per column', () => {
    const p = new Raster(16, 9);
    p.plot((t) => (t < 0.5 ? -1 : 1), { x0: 0, y0: 0, w: 16, h: 9 });
    expect(components(p)).toBe(1);
    for (let x = 0; x < 16; x += 1) {
      let any = false;
      for (let y = 0; y < 9; y += 1) any ||= p.get(x, y) === 1;
      expect(any).toBe(true);
    }
  });

  it('plots an exact 45 degree line as one pixel per column', () => {
    const p = new Raster(9, 9);
    p.plot((t) => 1 - 2 * t, { x0: 0, y0: 0, w: 9, h: 9 });
    expect(p.count()).toBe(9);
    for (let x = 0; x < 9; x += 1) expect(p.get(x, x)).toBe(1);
  });

  it('plots a dotted constant on even columns only', () => {
    const p = new Raster(9, 3);
    p.plot(() => 0, { x0: 0, y0: 0, w: 9, h: 3, dotted: true });
    expect(p.toRows()[1]).toBe('#.#.#.#.#');
    expect(p.count()).toBe(5);
  });

  it('counts clipped pixels instead of drawing them', () => {
    const p = new Raster(4, 4);
    p.line(-2, 0, 5, 0);
    expect(p.clipped).toBe(4);
    expect(p.count()).toBe(4);
  });
});

describe('renderGlyph', () => {
  it('merges equal neighbouring frames and maps every sample to a kept frame', () => {
    const r = renderGlyph({
      size: [4, 4],
      range: 'unipolar',
      frames: 5,
      draw: (p, v) => p.rect(0, 0, v < 0.4 ? 1 : 2, 1),
    });
    expect(r.frames.length).toBe(2);
    expect(r.valueMap).toEqual([0, 0, 1, 1, 1]);
  });

  it('renders transition frames for each ordered pair of states', () => {
    const r = renderGlyph({
      size: [4, 4],
      states: ['a', 'b'],
      draw: (p, i) => p.px(i, 0),
      transition: { frames: 2, draw: (p, from, to, t) => p.px(Math.round(from + (to - from) * t), 1) },
    });
    expect(r.transitions?.map((t) => [t.from, t.to, t.frames.length])).toEqual([
      [0, 1, 2],
      [1, 0, 2],
    ]);
  });

  it('builds transitions only for the listed pairs', () => {
    const r = renderGlyph({
      size: [4, 4],
      states: ['a', 'b', 'c'],
      draw: (p, i) => p.px(i, 0),
      transition: {
        frames: 2,
        pairs: [
          ['a', 'b'],
          ['b', 'a'],
        ],
        draw: (p, from, to, t) => p.px(Math.round(from + (to - from) * t), 1),
      },
    });
    expect(r.transitions?.map((t) => [t.from, t.to])).toEqual([
      [0, 1],
      [1, 0],
    ]);
  });
});

describe('set loading and check', () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
  });

  function makeSet(files: Record<string, string>): string {
    const dir = mkdtempSync(path.join(tmpdir(), 'dkglyph-'));
    dirs.push(dir);
    writeFileSync(path.join(dir, 'set.toml'), 'name = "t"\n[sizes]\nsmall = [3, 3]\n');
    writeFileSync(path.join(dir, 'catalogue.toml'), '[[glyph]]\nid = "a/gone"\nv1 = true\n');
    for (const [name, text] of Object.entries(files)) {
      mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
      writeFileSync(path.join(dir, name), text);
    }
    return dir;
  }

  it('isolates a broken glyph and still loads the others', async () => {
    const dir = makeSet({
      'a/ok.glyph.mjs': "export default { size: 'small', pixels: '###\\n#.#\\n###' };",
      'a/bad.glyph.mjs': "export default { size: 'small', draw() { throw new Error('boom'); } };",
    });
    const set = await loadSet(dir);
    const byId = Object.fromEntries(set.glyphs.map((g) => [g.id, g]));
    expect(byId['a/ok']?.rendered?.frames.length).toBe(1);
    expect(byId['a/bad']?.error?.message).toBe('boom');
    expect(byId['a/bad']?.error?.where).toMatch(/bad\.glyph\.mjs:\d+/);
  });

  it('reloads a glyph file after an edit', async () => {
    const dir = makeSet({ 'a/x.glyph.mjs': "export default { size: [2, 1], pixels: '#.' };" });
    expect((await loadSet(dir)).glyphs[0]?.rendered?.frames[0]?.toRows()).toEqual(['#.']);
    await new Promise((r) => setTimeout(r, 20));
    writeFileSync(path.join(dir, 'a/x.glyph.mjs'), "export default { size: [2, 1], pixels: '.#' };");
    expect((await loadSet(dir)).glyphs[0]?.rendered?.frames[0]?.toRows()).toEqual(['.#']);
  });

  it('reports a pixel rows size mismatch, clipping and missing catalogue entries', async () => {
    const dir = makeSet({
      'a/short.glyph.mjs': "export default { size: 'small', pixels: '###\\n###' };",
      'a/clip.glyph.mjs': "export default { size: 'small', draw(p) { p.rect(2, 2, 3, 3); } };",
    });
    const set = await loadSet(dir);
    const report = checkSet(set);
    expect(report.errors.some((e) => e.id === 'a/short' && /2 pixel rows/.test(e.text))).toBe(true);
    expect(report.errors.some((e) => e.id === 'a/clip' && /outside/.test(e.text))).toBe(true);
    expect(report.warnings.some((w) => w.id === 'a/short' && /catalogue/.test(w.text))).toBe(true);
    expect(report.missingV1).toEqual(['a/gone']);
  });

  it('reports a value glyph with a NaN frame count as an error', async () => {
    const dir = makeSet({
      'a/nan.glyph.mjs': "export default { size: 'small', range: 'unipolar', frames: NaN, draw(p) { p.px(0, 0); } };",
    });
    const report = checkSet(await loadSet(dir));
    expect(report.errors.some((e) => e.id === 'a/nan' && /frame count/.test(e.text))).toBe(true);
  });

  it('reports a set directory without set.toml as a set-level error', async () => {
    const dir = makeSet({});
    rmSync(path.join(dir, 'set.toml'));
    const report = checkSet(await loadSet(dir));
    expect(report.errors.some((e) => e.id === '(set)' && /set\.toml/.test(e.text))).toBe(true);
  });
});
