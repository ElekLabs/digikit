# Para

The track's sample played as a one-shot, paraphonically: each trig takes the next of four voices (1, 2, 3, 4, 1, ...), which keeps
that trig's note and has its own amp envelope. The one-shot counterpart of ParaWT (tom/para).

| knob | |
|---|---|
| A TUNE | stock; C4 plays the sample at its own pitch |
| B LOOP | bipolar: OFF in the middle (one-shot), right a forward loop, left ping-pong. One step either side loops the whole sample; further out moves the loop start towards the end |
| C DTUN | detune: voices 1-4 at +, -, +1/2, -1/2 of d³ semitones |
| D SMP | stock sample slot |
| E ATK | attack, 0 (instant) to 4 s, linear |
| F DEC | decay after the trig's note-off, 5 ms to 10 s to -60 dB |
| G STRT | where in the sample a voice starts (read at its trig) |
| H SPRD | stereo spread: voices 1-4 at right, left, right/2, left/2. The track's level is fixed at 100 |

- **Chords:** as ParaWT: a trig records up to four notes, as on a MIDI track; each note takes the next voice.
- **Envelope:** as ParaWT: attack from the voice's level, held for the trig, decay after the note-off. Without a loop a voice
  also ends at the sample's end.
- **AMP page:** the track's own amp envelope multiplies the sum and restarts at every trig: set it open (attack 0, sustain
  full, a long release).
- **Level:** each voice 6 dB below a stock sample machine playing the slot, summed.
- **Stereo** samples play in stereo.

Cost: about 19,400 instructions per block with four voices on a stereo sample (about 4 % of the DSP per track), 5,800 for one.

Type 22.
