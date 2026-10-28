# TB303

Open303 as a Digitakt II machine: the 303 saw or square through Open303's TB-303 filter, its filter envelope and accent, and
its amp envelope and output stage. Set the track's AMP page to a gate (attack 0, hold or decay long, sustain full, release
short): it still multiplies the machine and retriggers on every trig. The FLTR page still filters after the machine.

| knob | |
|---|---|
| A TUNE | stock |
| B WAVE | SAW or SQR |
| C CUT | cutoff, 314 to 2394 Hz (Open303's knob) |
| E RES | resonance |
| F ENV | env mod |
| G DEC | filter envelope decay, 200 to 2000 ms |
| H ACC | accent amount, 0 to 100 % (Open303's accent knob) |

- **Accent:** a trig with velocity above 100. ACC sets how much: the accent sweep and the louder amp scale with it; the 200 ms
  filter decay and the 50 ms release apply to every accented note, as in Open303.
- **Level:** knob H is the slot the stock level reads, so TB303 tracks play at a fixed LEV of 100. AMP VOL and the kit's track
  level still set the volume. The DSP's frame unpack (`FUN_1c24e9`) is called through `_tb_unpack`, which hands it 100 on a
  TB303 track; the frame copy made later in the block restores the knob, so the machine reads ACC. Old sounds read their LEV as ACC.
- **Slide (a tie):** a note slides from the previous pitch when the previous note is still held as it starts (Open303's 60 ms
  slide). Neither the amp nor the filter envelope restarts; the new note sets the accent, the filter decay and the release.
  In the sequencer that means a trig whose LEN reaches the next trig. Set the track's LEN below 1 step (for example 0.5, a
  303's gate) and lengthen the trigs that should slide. Live in KEYBOARD mode: play legato.

Differences from Open303:

- Runs at 48 kHz without Open303's 4x oversampling and its elliptic decimator. The oscillator level is the table increment's
  exponent + 1 (Open303: + 2), and nothing above 24 kHz is made. Against Open303 itself (`tb303_ref.Open303`, checked against
  its C++ to 1e-7) the spectra differ by 0.1 to 0.5 dB (log-spectral distance, test_tb303.py).
- b0's division is a reciprocal by three Newton steps from a quadratic guess (2e-7).
- Cutoff, env mod and resonance glide to the knob values with a 2 ms time constant. Abrupt changes every block (an LFO's square,
  p-locks) make Open303's filter blow up, at 4x too; the glides keep it bounded.
- The amp is Open303's: its envelope (instant attack, 1230 ms decay to 0 while held, 1 ms release, 50 ms after an accent) plus
  (0.45 + 4 accent) env while held, through its declicker. Output gain -6 dB (the plug-in's default volume).
- One voice per track; the phase, the filters and the envelope run on across notes (Open303 resets them only when idle).
- Levels 4-11 of the waveforms are stored shorter (exact resamplings; 83 KB instead of 197 KB).

The slide and the amp's release read the stock release latch (`dsp.voice.release_latch`, core/dsp-voice): a one-block pulse at a
note-off, which the routine ORs into its state until the next note-on.

Cost: about 10,500 instructions per block including the adapter, roughly 2 % of the DSP per TB303 track.

Tables and constants: `uv run python modules/machines/tb303/src/dsp/gen_tables.py` writes `src/dsp/tables.inc`. Model:
`tools/patchkit/tb303_ref.py` (`Tb303`); the routine matches it exactly on the SHARC core (`tests/test_tb303.py`).

Image: `mise run patch -- --firmware firmware/Digitakt_II_OS1.17.syx --modules tom/tb303 -o out/patch/tb303-dt2-1.17.syx`.

Open303 is Copyright (c) 2009 Robin Schmidt, MIT licence: see `LICENSE-Open303.txt`. The SAW303 and SQUARE303 level tables in
`src/dsp/tables.inc` and the filter, envelope and accent models come from it.
