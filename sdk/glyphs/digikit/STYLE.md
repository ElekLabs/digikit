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
| `panel` | 51x17 | Wide picture: filter response, envelope, waveform | Stock filter graph (51x17) |

A glyph uses one class. Content keeps the outer 1 px ring of the cell empty when the glyph is `invertible`, except `knob` ring glyphs, which may touch the cell edge as the stock knob does.

## Strokes and shapes

- Strokes are 1 px. Two-pixel strokes only for a solid fill that represents "on" or "amount".
- Use whole-pixel geometry from the raster helpers (`line`, `arc`, `circle`, `plot`, `rect`, `frame`). Never draw with anti-aliasing.
- Lines at 45 degrees and axis-aligned lines read best. Avoid slopes between 1:3 and 3:1 except in curves.
- Dotted lines (every other pixel) mean secondary: guides, scale ticks, empty slots, ghosts of the previous state.
- Corners of filled bars and frames larger than 5 px are cut by one pixel (as the stock selection bar).
- Solid fill vs outline: a filled shape means "active, on, present"; the same shape as an outline means "inactive, off, empty".
- Small node squares are 3x3 with a hollow centre (stock envelope nodes).
- No text inside glyphs except where the glyph is a label tile; text then uses the 3x5 digits and letters from `font/`.

## Value glyphs (encoders)

- Knob: 17x17 ring (circle radius 8, centre 8,8), 1 px pointer line from the centre to the ring.
  - Sweep 270 degrees with the gap at the bottom: minimum at 7:30 (225 deg), maximum at 4:30 (-45 deg), clockwise.
  - Unipolar: no tick. Bipolar: 1 px tick at 12 o'clock inside the ring, and 3 px `-` and `+` marks beside the bottom gap.
  - Frames: 25 for 0..127 (one frame per distinct pointer position; the build merges identical frames).
- Other value glyphs (fader, bar, pan, tune, width, amount) follow the same rule: one moving element on a fixed, mostly dotted scale.
- Value glyphs that change a shape (pulse width, fold, resonance, curve) draw the shape itself at the value; keep the frame on the same baseline so the shape morphs in place.

## State glyphs (toggles and enums)

- Each state is a complete glyph on its own; the transition frames are optional extras.
- Transitions are 2 to 4 frames and move one element (a knob slides, a fill grows). No transition flips the whole glyph.
- On/off pairs differ by fill (solid vs outline), never by position alone.
- Enum glyphs in a parameter slot (play mode, waveform, filter type) are `knob` class tiles: the picture centred in 17x17, no ring.

## Waveforms and curves

- Draw with `plot(fn)` across the full content width so every waveform shares stroke and baseline.
- A waveform shows one period, starting at the zero crossing unless the shape has none (ramp, saw).
- Vertical extent: full content height minus the margin; the centre line is not drawn.
- Random or noise shapes are fixed patterns (seeded), never random per build.

## Families

- Same concept, same picture: the sine in an LFO list, an oscillator slot and a filter mode uses the same curve at the size of its class.
- A family shares a base picture and varies one feature (loop arrow added, direction mirrored, fill toggled).

## Naming

- File path: `<group>/<name>.glyph.mjs`. Group is one of the catalogue groups. Names are lower case, short, and name the concept (`wave/saw-down`, `play/rev-loop`), never the module that uses them.
