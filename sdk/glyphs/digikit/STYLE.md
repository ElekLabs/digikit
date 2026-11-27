# digikit glyph set: style

Rules for every glyph in this set. An agent that draws a glyph reads this file first and follows it exactly. The set must sit beside the stock firmware screens without looking foreign, and every glyph must read at 1x on a 128x64 1-bit OLED.

## Screen facts

- 1 bit per pixel. No grey, no dithering for shading. On pixels are light on a dark panel.
- Emphasis is shown by inversion (selected menu row, locked parameter). Every glyph must still read when inverted.
- Stock text: cap height 5 px, letters 4 px wide (3 px for narrow letters), 1 px letter gap.

## Size classes

| Class | Size | Use | Stock reference |
| --- | --- | --- | --- |
| `knob` | 17x17 | Value glyph in a parameter slot (one of 8 slots, x pitch 26, y pitch 27) | Stock knob ring, dotted empty slot |
| `icon` | 9x9 | Menu row icon, list markers, module kinds | Stock settings menu icons (about 9x8) |
| `inline` | 7x5 | Inside a text line, beside 5 px caps | Stock tempo and lock marks |
| `digit` | 3x5 | Digits in label tiles and value readouts | Stock 5 px UI font |
| `panel` | 51x17 | Wide picture: filter response, envelope, waveform | Stock filter graph (51x17) |

A glyph uses one class. The empty outer ring per class is in `set.toml` `[margins]`: `knob` and `panel` keep 1 px empty (a ring knob may touch the cell edge as the stock knob does); `icon`, `inline` and `digit` use the full cell, because the context gives the inverted border (the 11 px selection bar around a 9 px icon, the bar padding around 5 px text).

## Strokes and shapes

- Strokes are 1 px. Two-pixel strokes only for a solid fill that represents "on" or "amount".
- Use whole-pixel geometry from the raster helpers (`line`, `arc`, `circle`, `plot`, `rect`, `frame`). Never draw with anti-aliasing.
- Lines at 45 degrees and axis-aligned lines read best. Avoid slopes between 1:3 and 3:1 except in curves.
- Dotted lines (every other pixel) mean secondary: guides, scale ticks, empty slots, ghosts of the previous state.
- A dotted copy of a curve (ghost, previous state, dry signal, second voice) uses `plot(fn, { ...box, dotted: true })`: even columns only, no joins. Never hand-roll a dotted curve.
- Markers on a curve (trigger point, start phase) are a 3x2 solid triangle pointing down at the point, just above the curve (rows `###` and `.#.`), or a 3x3 hollow node on the curve. Never a T shape.
- Corners of filled bars and frames larger than 5 px are cut by one pixel (as the stock selection bar).
- Solid fill vs outline: a filled shape means "active, on, present"; the same shape as an outline means "inactive, off, empty".
- Small node squares are 3x3 with a hollow centre (stock envelope nodes).
- No text inside glyphs except where the glyph is a label tile; text then uses the 3x5 digits and letters from `font/`.

## Icons (9x9)

- Use the full 9x9 cell, as the stock settings icons do.
- Silhouette first: an icon must be recognisable from its outline alone at 1x. Icons that appear side by side (play, stop, rec; the module kinds) differ in outline, not only in interior detail.
- One solid mass plus one outline or cut-out detail reads best (stock: a filled body with a 1 px cut-out). Interior detail is 1 px lines with 1 px gaps, never finer.
- A disc in 9x9 is the 9x9 circle with flat 3 px sides (rows/columns 0 and 8 hold pixels 3..5 only); a square is the full 9x9 or 7x7 block, so the two never look alike.

## Value glyphs (encoders)

- Knob: 17x17 ring (circle radius 8, centre 8,8), 1 px pointer line from the centre to the ring.
  - Sweep 270 degrees with the gap at the bottom: minimum at 7:30 (225 deg), maximum at 4:30 (-45 deg), clockwise.
  - Unipolar: no tick. Bipolar: a dotted tick at 12 o'clock inside the ring (pixels at rows 1 and 3 of column 8), and 3 px `-` and `+` marks at row 15 beside the bottom gap.
  - Frames: 25 for 0..127 (one frame per distinct pointer position; the build merges identical frames).
- Other value glyphs (fader, bar, pan, tune, width, amount) follow the same rule: one moving element on a fixed, mostly dotted scale.
- Bipolar value tiles that are not knobs (bend, curve, depth, freq shift) have no ring and no `-`/`+` marks; v = 0 is the neutral picture.
- Value glyphs that change a shape (pulse width, fold, resonance, curve) draw the shape itself at the value; keep the frame on the same baseline so the shape morphs in place.

## State glyphs (toggles and enums)

- Each state is a complete glyph on its own; the transition frames are optional extras.
- Transitions are 2 to 4 frames and move one element (a knob slides, a fill grows). No transition flips the whole glyph.
- Direction change (an arrow that reverses, e.g. rev-loop to fwd-loop): the arrow squeezes to a vertical line and reopens the other way; this is the one transition that changes the whole glyph.
- On/off pairs differ by fill (solid vs outline), never by position alone.
- List the pairs the UI can step between in `transition.pairs` (for an enum: neighbours in both directions). Pairs that are never shown are not drawn.
- Enum glyphs in a parameter slot (play mode, waveform, filter type) are `knob` class tiles: the picture centred in 17x17, no ring.

## Waveforms and curves

- Draw with `plot(fn)` across the full content width so every waveform shares stroke and baseline.
- A waveform shows one period, starting at the zero crossing unless the shape has none (ramp, saw).
- In a knob tile, a waveform uses the box x 1..15, y 4..12 (15 columns, 9 rows, centre row 8) with `period: 16`, so 45 degree slopes are exact; the shared box is `BOX` in `wave/shape.mjs`.
- Draw the character, not the samples: when 15 columns cannot show a feature (a ripple, a second hump, a third harmonic), exaggerate it or adjust the period until it reads at 1x, and keep the shape recognisable as the module's shape.
- Random or noise shapes are fixed patterns (seeded), never random per build.

## Families

- Same concept, same picture: the sine in an LFO list, an oscillator slot and a filter mode uses the same curve at the size of its class.
- A family shares a base picture and varies one feature (loop arrow added, direction mirrored, fill toggled).

## Naming

- File path: `<group>/<name>.glyph.mjs`. Group is one of the catalogue groups. Names are lower case, short, and name the concept (`wave/saw-down`, `play/rev-loop`), never the module that uses them.
