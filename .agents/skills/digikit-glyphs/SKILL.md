---
name: digikit-glyphs
description: Draw, check and preview 1-bit digikit glyphs (icons, knob tiles, animated value and state glyphs) as .glyph.mjs files in an SDK glyph set such as sdk/glyphs/digikit, with mise run glyphs (check, sheet, json, dev). Use for new glyphs, glyph sets, redrawing a glyph, the glyph STYLE.md rules and the catalogue.
---

# digikit glyphs

Run every command from the repository root. Tool details: `packages/dkglyph/README.md`.

## Before drawing

Read, in full: the set's `STYLE.md` (the rules; follow them exactly), `set.toml` (size classes and margins), `catalogue.toml` (ids, groups, notes), and the accepted glyphs next to the one you draw, so it joins their family. Reuse the set's helper modules instead of copying code: `wave/shape.mjs` (`BOX`, `wavePlot`, `sine`, `tri`, `seeded`, `colPlot`), `filter/shape.mjs` (`baseline`, `lowPass`, `poly`), `env/node.mjs` (`node`), `play/arrow.mjs` (play-mode arrows). Only `*.glyph.mjs` files load as glyphs; any other `.mjs` in the set is a helper.

## Glyph file

`<group>/<name>.glyph.mjs` exports one definition:

- static: `pixels` (rows of `#` and `.`) or `draw(p)`.
- value: `range` (`unipolar` or `bipolar`), `frames`, `draw(p, v)`; `v` is 0..1 or -1..1.
- states: `states` (names), `pixels` per state or `draw(p, i)`, optional `transition: { frames, pairs, draw(p, from, to, t) }`.

Common fields: `title`, `size` (a class name from `set.toml`), `tags`, `invertible`. `p` is a 1-bit raster with integer coordinates and no anti-aliasing: `px`, `line`, `rect`, `frame`, `circle`, `arc` (0 degrees is right, angles grow clockwise), `poly`, `plot(fn, box)` (`box.dotted` for a dotted copy, `box.period` for exact periods), `invert`, `flipX`, `flipY`, `blit`. Prefer `pixels` grids for 9x9 icons; use geometry for curves and value glyphs.

## Loop

1. Write or edit the glyph file. Add its id to `catalogue.toml` if it is new.
2. `mise run glyphs -- check` (or `node packages/dkglyph/cli/glyphs.mjs check sdk/glyphs/digikit`): fix every error and warning in your files.
3. `mise run glyphs -- sheet -o <png>` and read the PNG; for a closer look, `node packages/dkglyph/cli/glyphs.mjs json sdk/glyphs/digikit` gives every frame as packed bits.
4. Judge at 1x and enlarged: readable from its silhouette, clean 1 px strokes, smooth frames, same weight as its neighbours. Repeat.

`mise run glyphs -- dev` opens a browser preview that updates when any file in the set changes: scale, pixel grid, inverted look, value slider, state buttons, and a 128x64 mock screen.

Glyphs are not yet drawn on the device: modules cannot use them until the SDK canvas has a bitmap call.
