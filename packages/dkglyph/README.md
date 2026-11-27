# dkglyph: glyph authoring

Glyphs are 1-bit pixel icons and animated value glyphs (knobs, switches, waveforms) kept as source files in the SDK under `sdk/glyphs/<set>/`. The tool builds frames from them, checks them and shows them in a preview that reloads when a glyph file changes. It has no device integration and no C output yet.

## A set

A set is a directory:

```text
sdk/glyphs/digikit/
  set.toml            name, title, description, [sizes] classname = [w, h]
  catalogue.toml      [[glyph]] id, group, v1 = true|false, note  (the glyphs the set should have)
  STYLE.md            drawing rules
  control/knob.glyph.mjs
```

Every `**/*.glyph.mjs` is a glyph. Its id is the path without the suffix (`control/knob`) and its group is the first path segment. A file exports a definition as `default`; `glyph()` from `@digi/dkglyph` only types it. `draw` functions get a `Raster` (`px line rect frame circle arc poly plot invert flipX flipY blit`). Angles in `arc` start at the right and grow clockwise on the screen. One broken file shows as an error and never stops the rest of the set.

Size is `[w, h]` or the name of a class in `set.toml`. Common optional fields: `title`, `tags`, `invertible` (default true: the glyph is meant to be shown inverted on a selection bar).

### Static

```js
export default { size: [3, 3], pixels: `
  ###
  #.#
  ###
` };
// or: draw(p) { p.plot((t) => Math.sin(2 * Math.PI * t), { x0: 1, y0: 1, w: 13, h: 7 }); }
```

### Value

```js
export default { size: 'knob', range: 'unipolar', frames: 25, draw(p, v) { /* v in [0,1], bipolar: [-1,1] */ } };
```

The frames are sampled evenly over the range; equal neighbours merge and `valueMap` says which kept frame each sample uses.

### States

```js
export default {
  size: [13, 7],
  states: ['off', 'on'],
  draw(p, i) { /* state i */ },            // or pixels: ['...', '...'] per state
  transition: { frames: 3, draw(p, from, to, t) { /* t in (0,1) */ } },
};
```

## Commands

From a clean checkout (`pnpm install` once). SET defaults to `sdk/glyphs/digikit`.

```sh
mise run glyphs -- check [SET]            # errors, warnings, coverage; exit 1 on errors
mise run glyphs -- sheet [SET] [-o PNG]   # contact sheet, default out/glyphs/<set>.png
mise run glyphs -- dev [SET] [--no-open] [--port N]   # live preview
```

`check` prints one line per message and then the totals:

```text
digikit: 3 glyph files, 3 built, 0 errors, 3 warnings
coverage: 0 of 0 catalogue entries have a file
```

Errors: pixel rows not matching the size, draws outside the raster, duplicate ids, fewer than 2 value frames, an empty frame, a glyph that fails to load. Warnings: an invertible glyph touching its 1-pixel outer margin, a glyph missing from `catalogue.toml`. Coverage lists catalogue ids without a file, v1 ones first.

`dev` prints the local URL (opened in the browser unless `--no-open`). The page shows the set by group at 1x to 8x with optional pixel grid and inverted look, animates value and states glyphs, and has a detail panel (16x view, frames, value slider, state buttons with transitions, check messages) plus a mock 128x64 screen with a parameter page and a menu row. The mock layout is in `preview/layout.ts` and is a placeholder until measured. Editing any file in the set reloads it without a page refresh. `GET /set.json` returns the built set.

`export.ts` also writes the set as JSON (`setJson`): per glyph `frames` are hex strings of `w*h` bits, row-major, MSB first.
