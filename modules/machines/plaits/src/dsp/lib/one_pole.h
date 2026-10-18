/* PLAITS for the Digitakt II: the state of stmlib's OnePole filter (stmlib/dsp/filter.h, MIT, Emilie Gillet), apart
 * from its functions (lib/units.h) so that an engine's header can hold one without pulling in the static helpers. */
#ifndef PLAITS_LIB_ONE_POLE_H
#define PLAITS_LIB_ONE_POLE_H
typedef struct { float g_, gi_, state_; } OnePole;
#endif
