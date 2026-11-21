// digikit/freqshift: the parameter words of the effect, shared by src/module.c and src/dsp/freqshift.s.
// Only `//` comments on their own line and decimal `#define`s: the SHARC+ assembler keeps a trailing comment in the
// macro text and upper-cases it.

// Parameter word p0: the shift, -64 .. +63.
#define FSHIFT_SHIFT_WORD 0
// Parameter words the effect reads.
#define FSHIFT_KNOBS 1
