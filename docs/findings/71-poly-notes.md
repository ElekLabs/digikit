# 71. Up to four notes per trig on audio tracks (DT2 1.17, 2026-10-29)

The facts behind `modules/core/polynotes`. Marks: **[D]** read or measured once by the author; nothing here is **[V]**.

## The note pool **[D]**

- Every step of every track (all 16) has a list of up to four notes in the pattern: node array at pattern `+0x6a50`, 6-byte nodes
  `{track, step, note, velocity, length, ?}`, the s16 head of `(track, step)` at node array `+0xc000 + 2 (track * 0x80 + step)`, the
  s16 next of node i at node array `+0xd000 + 2 i`. Note 1 is absolute; notes 2-4 are `0x40 + note - note1`, 0xff none.
- The slot setters are `0x4005919e` `0x40059214` `0x4005928c` `0x40059304` (note 1-4), the list reader `0x400399f8`.
- MIDI tracks play every note (`0x4012c590`, one note-on each). The trig builder for audio tracks (`0x400d6982`) hands the whole
  list to the trig event (event `+0x1c` node array, `+0x20` head), but `vector_191_handler` reads note 1 alone.

## Recording **[D]**

The live-record note writer `0x40048fb2` (from the note handler `0x400318c6`) and the other note writer `0x40030d12` add notes
2-4 only when `FUN_4003630c` (is audio: the inverse of the kit's per-track MIDI bit at `+0x552e`) says no. The MIDI bit also
routes the track to MIDI, so core/polynotes replaces the two calls (`0x40049058`, `0x40030d5a`) instead.

Emulator, Chord (since retired: Para and ParaWT replace it) on track 1, stock sample machine on track 2, live record, C4 E4 G4 sent at once on both tracks' channels:
track 1 step 3 holds 60, 0x44, 0x47 (the note-off writes the length into all of them); track 2 holds 67 alone, as stock.
`0x40030d12` was not exercised.

## Playback **[D]**

`pn_trig` hooks `vector_191_handler` at `0x4002e5dc` (a new trig on an audio track, after its sound and locks; event at
`(-0x64,A6)`, track in D2). For a poly track it writes notes 2-4 into the TX frame long at `+0xf4 + 0x60 t`, one of MANUAL
SLICE's slice words (finding 70), as halfwords `{note3 << 8 | note2, note4}` (0x40 + the interval, 0 none, so a frame never written reads as one note); a trig with one note writes 0.
The DSP's frame copy (`dsp.voice.frame_base`) has them in word 61 as `note2 | note3 << 8 | note4 << 16`, in the same frame as
the trig. Measured: `0x00ff4744` for the chord above (before none became 0) in word 61 on track 1 and word 85 on track 2: the
DSP's track stride is 0x18 words (0x60 bytes), as core/trackflags' readers assume.

## The machines **[D]**

tom/para (ParaWT) and tom/parasmp (Para) read word 61 in note_on: note 1 and each of notes 2-4 that is not 0, 0x40 or 0x80 up
take the next slot (round robin, the oldest stolen) at note 1's pitch + 256 x the interval. Rig tests against the models
(`note_on_notes`) pass. Emulator, Para on track 1, the chord above played back: three slots start held at full level, the
fourth is untouched, `S_NEXT` = 3; after the note-off the three decay. The emulator runs a changed DSP image in the
interpreter (about 30 x slower than real time): a probe of a few seconds takes some 15 minutes.

**[O]** the trig page (`TrigParameterSet::vfunc_27` @`0x4003d112` uses the same test to
show NOTE 2-4); keyboard and MIDI notes outside recording (they reach the dispatch as separate trigs with one note); a pattern
with chords on an audio track on the stock OS.
