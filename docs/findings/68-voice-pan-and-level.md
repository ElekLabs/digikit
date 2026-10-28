# Voice pan stage and the per-voice parameter records (DT2 1.17)

**[D]** read once from the image and measured on the emulator (2026-10-27); a second-agent check is still due.

## Parameter records

The per-voice frame unpack `FUN_1c24e9` (called twice per track every block by `FUN_1c2b24` at `0x1c2c9a` and `0x1c2ca9`, R12 =
track, R8 = voice, unconditionally) writes one record per voice at byte `0x2506ec + 0xdc v` (55 words; anchor `dsp.voice.params`,
the literal of `I5 = MODIFY(I4, 0x2506ec)` at `0x1c25ca`). Word 52 is the voice's pan, 0 .. 1 (`pantab[pan_hw >> 8]`, table at
`0x256798`); word 53 holds the same value. Both voices of a track get the track's pan.

## Pan stage

`FUN_1c642a`'s 16-pass loop at `0x1c7040` mixes each track's two voice buffers (the pointer table at `0x24ef2c`: voice 2t, 2t + 1)
through a 2 x 2 gain matrix (`0x1c71ec`): out_L = gL(p0) v0 + gL(p1) v1, out_R = gR(p0) v0 + gR(p1) v1, where p0 and p1 are word
52 of the two records x 127, interpolated in two 128-entry tables at DDR `0x8055c440` (gL, cos) and `0x8055c640` (gR, sin):
equal power, 0.7071 each at the centre. A mono machine's odd voice is a copy of its even voice, so the pair sums to 1.414 x the
voice in each channel. With both pans at the centre two different voice blocks come out identical in L and R; core/dsp-voice
therefore moves a stereo row's voices to pan - 1/2 and pan + 1/2 (clamped) on the odd voice's call, after the unpack and
before the pan stage. Measured: GND at the centre gives words 52 of 0.0 and 1.0 and an L/R correlation of 0.675.

The path selector before it (`R2 = DM(I15)`, `leftz`, `0x1c7051`) reads a table refilled from record word 54 of both voices; it
was 0 for every track measured, which takes the matrix path.

## Knob H and the level

The unpack turns knob H's frame halfword (`0x76 + 48 t`, the low half of the word at byte `0xec + 0x60 t` of the frame copy
`0x2558dc`) into the voice chain's level. The per-block frame copy after the unpack writes the knob back before the voices render.
core/dsp-voice retargets both unpack calls to `_dv_unpack`, which writes a row's `lev` word there first (anchors
`dsp.voice.unpack.a`, `.b`, `dsp.voice.unpack`): the track keeps a fixed level and the machine gets knob H as a parameter.
