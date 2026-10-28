# Chord

The track's sample played as a one-shot by up to four voices at set intervals: a single piano or string note becomes a chord.
The one-shot counterpart of ChordWT (tom/wtstack).

| knob | |
|---|---|
| A TUNE | stock; C4 plays the sample at its own pitch |
| B LOOP | bipolar: OFF in the middle (one-shot); right of it a forward loop, left of it ping-pong. One step either side loops the whole sample; further out moves the loop start towards the end |
| C DTUN | detune, as ChordWT: the sounding voices go alternately up and down by d³ semitones (±100 cents at the ends) |
| D SMP | stock sample slot |
| E INT2 | voice 2, -24 .. +24 semitones from the note, UNI (the note itself) one step above OFF |
| F INT3 | voice 3, as INT2 |
| G INT4 | voice 4, as INT2 |
| H SPRD | stereo spread, as ChordWT. The track's level is fixed at 100 |

- **Voices:** all start at the sample's first frame on a trig. A voice whose INT is turned to OFF ends until the next trig.
- **Stereo:** a stereo sample plays in stereo, each voice panned by SPRD as a balance.
- **Level:** one voice at the centre plays as loud as a stock sample machine plays the slot; n voices at 1/√n each.
- **End:** without a loop each voice ends at the sample's end; the note ends when all have. The track's AMP envelope shapes
  the whole chord.
- **Loops** jump without a crossfade, so a loop point in the middle of a waveform clicks; ping-pong turns at the ends.

Cost: about 3,900 instructions per block for one mono voice, 17,800 for four voices on a stereo sample (about 3.7 % of the DSP
per track), including the adapter.

Type 21.
