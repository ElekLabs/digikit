# Bass Drum

**Category:** machines  **Devices:** Digitakt II 1.17; Digitone II 1.12  **Status:** experimental (emulator only)  **Kind:** run-time module (`.dkm`, loaded from the card; the runtime gives it its machine type)

An FM kick drum. A carrier sine is phase-modulated by a second sine. The modulation depth (the index) starts high and
falls fast, so the strike has a burst of sidebands that settles into a plain sine. The carrier pitch also starts high
and falls to the note. The amplitude decay is the track's AMP envelope, so shape the body and the tail there: the
machine only has to sound right for the first moments of the note.

| knob | range |
|---|---|
| **TUNE** | the note (stock knob, slot 0): the pitch the kick settles on |
| **BEND** | 0 = no pitch envelope · full = the pitch starts at 3.5 times the note and falls to it over about 27 ms |
| **INDEX** | 0 = a plain sine · full = a phase swing of up to 3 cycles at the strike, falling to 0 over about 75 ms |
| **RATIO** | modulator frequency as a multiple of the carrier, 0.25 times (0) to 4 times (full). About 0.2 is 1:1 (smooth), about 0.47 is 2:1 (bright). Other values are inharmonic and metallic |
| **NOISE** | 0 = none · full = a white-noise burst with its own envelope of 40 ms, for a snare or hat character |

Turn the AMP decay down for a kick. A high INDEX with a low RATIO gives a heavy thud; a high RATIO gives a bell-like
clang. A low BEND with a long AMP decay gives a sub.

The machine keeps its own block counter, which is zeroed at every note-on. It drives all the envelopes, so the first
trig after power-on sounds the same as any later one.
