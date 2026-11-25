// digikit/freqshift: the parameter words of the effect, shared by src/module.c and src/dsp/freqshift.s.
// Only `//` comments on their own line and decimal `#define`s: the SHARC+ assembler keeps a trailing comment in the
// macro text and upper-cases it.

// Parameter word p0: the shift, -64 .. +63.
#define FSHIFT_SHIFT_WORD 0
// Parameter word p1: stereo frequency offset in Hz, 0 .. 8.
#define FSHIFT_SPREAD_WORD 1
// Parameter word p2: bipolar modulation depth in Hz, 0 .. 4.
#define FSHIFT_DRIFT_WORD 2
// Parameter word p3: modulation frequency in Hz, 1/32 .. 1/8.
#define FSHIFT_RATE_WORD 3
// Parameter words the effect reads.
#define FSHIFT_KNOBS 4
