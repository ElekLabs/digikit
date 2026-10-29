# PARA

"ParaWT" in the machine menu. The track's sample as a Serum wavetable, played paraphonically: each trig takes the next of four voices (1, 2, 3, 4, 1, ...),
which keeps that trig's note and has its own amp envelope, so up to four notes ring together.

| knob | |
|---|---|
| A TUNE | stock; each voice plays its trig's note |
| B WAVE | position in the table: crossfades between neighbouring frames. FUNC + press WAVE switches SMOOTHING off (the nearer frame alone, stepping through the table) and on; kept with the sound (core/trackflags) |
| C DTUN | detune, -64 .. +63: voices 1-4 at +, -, +1/2, -1/2 of d³ semitones (d = 1 at +63: ±100 cents) |
| D SMP | stock sample slot: the wavetable |
| E ATK | attack, 0 (instant) to 4 s, linear |
| F DEC | decay after the trig's note-off, 5 ms to 10 s to -60 dB; the voice stops at -80 dB |
| G JUMP | wavetable jump, -64 .. +63: voice k (0-3) plays WAVE + k x JUMP frames, wrapped around the table |
| H SPRD | stereo spread: voices 1-4 at right, left, right/2, left/2 of the track's pan (equal power). The track's level is fixed at 100 |

- **Chords:** a trig records up to four notes, as on a MIDI track (live recording: play the notes together; core/polynotes).
  Each note takes the next voice, so a four-note trig takes all four and a chord after it steals the oldest.
- **Envelope:** attack from the voice's current level to full, held while the trig is held (until the track's note-off),
  then exponential decay. A note-off releases every held voice. A trig with LEN INF never releases: its voice holds until
  the fourth trig after it takes the voice again.
- **AMP page:** the track's own amp envelope still multiplies the sum and restarts at every trig. Set it open, attack 0 and
  sustain full, with the release long enough for DEC's tails, so it doesn't cut the other voices.
- **Trig:** the voice's phase restarts at the frame's start; a voice taken while still sounding attacks from its level.
- **Table:** frames of 2048 samples (Serum's), a shorter sample is one single-cycle frame; left channel only;
  no band-limiting; the table must reach the Digitakt at 48 kHz.
- **Output:** each voice at -6 dB, summed: one note is 6 dB below a full-scale stock
  sample, four held notes peak at most 6 dB above it.

Cost: about 11,600 instructions per block with four voices sounding, including the adapter (about 2.4 % of the DSP per
track); a voice that has stopped costs nothing.

Type 19.
