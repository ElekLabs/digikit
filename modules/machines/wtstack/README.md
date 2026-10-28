# WTStack

"ChordWT" in the machine menu. The track's sample played as a Serum wavetable by up to four voices at set intervals.

| knob | |
|---|---|
| A TUNE | stock; the pitch follows the trig's note |
| B WAVE | position in the table: crossfades between neighbouring frames |
| C DTUN | detune, -64 .. +63: the active voices go alternately up and down by d³ semitones (d = 1 at +63), so ±100 cents at the ends and under a cent within ±16 of the centre |
| D SMP | stock sample slot: the wavetable |
| E INT2 | voice 2, -24 .. +24 semitones from the note; OFF at 0 |
| F INT3 | voice 3, as INT2 |
| G INT4 | voice 4, as INT2 |
| H SPRD | stereo spread: the active voices go alternately left and right of the track's pan (equal power); a single voice stays centred. The track's level is fixed at 100 |

- **Table:** frames of 2048 samples (Serum's default), as many as the sample holds; a sample shorter than 2048 is one frame of
  its own length (a single-cycle wave). Only the left channel of a stereo sample is read. Each frame is read with linear
  interpolation; no band-limiting, so high notes on bright frames alias.
- **Voices:** voice 1 plays the note; INT2-INT4 add a voice each when not OFF. Detune and spread alternate over the voices that
  are on, in order (1 up and left, the next down and right, ...). Each voice is scaled by 2/√n for n voices.
- **Trig:** every voice's phase restarts at the frame's start.
- **Sample import:** the frame boundaries are sample counts, so the table must reach the Digitakt unresampled; a table at another
  rate than 48 kHz that the import converts no longer has 2048-sample frames.

Voice record: word 0 (the sample's first frame, a byte address) and +0x188 (its frames) of the even voice, through core/dsp-voice's
`_dv_g`. The selas assembler drops a load's `(SWSE)`, so the voice reads the 32-bit word holding a sample and masks out its half.

Cost: about 10,000 instructions per block with four voices including the adapter, about 1.5 % of the DSP per track.

Type 23.
