// =====================================================================
//  bd : an FM bass drum for the DT2 SDK contract
// ---------------------------------------------------------------------
//  Phase modulation, not an 808. A carrier sine is modulated by a second sine whose *index* decays fast, so the strike
//  is a clang of sidebands that collapses into a pure sine as the note decays. The carrier also carries a pitch
//  envelope, so the whole thing punches downward while the clang rings out. A sample cannot do that; this is the
//  instrument people mean when they ask for an FM kick.
//
//    carrier   f(t) = f0 * (1 + 2.5 * BEND * env^2)       env falls over 40 blocks, about 58 ms
//    modulator f(t) = carrier * (0.25 + 3.75 * RATIO)     1:1 smooth, 1:2 bright, non-integer ratios metallic
//    index     I(t) = 8 * INDEX * (1 - n/20)^2            modulation depth, gone in about 29 ms
//    out       sin(2 pi (carrier phase + I * sin(2 pi modulator phase)))
//
//  The amplitude decay is the track's AMP envelope, so the machine only has to sound right for the first 50 ms.
//
//  State: carrier phase, block counter, modulator phase (all zeroed at note-on, which is the envelope trigger), the
//  index, and the modulator's last value. Nothing survives a trigger, so there is nothing to seed.
//
//  Register map (F and R are one file):
//    F13 carrier phase   F15 modulator phase   F12 carrier inc   F14 modulator inc
//    F7 base inc   F10 ratio   F9 level   F6 BEND   F5 INDEX   F11 1.0   F4 0.5   F8 0.25
//    per-sample scratch: F0 F1 F2 F3
// =====================================================================
#include "dk_machine.inc"

.SECTION/PM seg_pmco;
.GLOBAL _bd_render;
.GLOBAL _bd_note_on;
.EXTERN _dv_exp2;

_bd_render:
    I3 = R4;
    I2 = R8;
    I1 = R12;

    // ---- pitch -> base increment ----
    R0 = DM(DK_IN_W_PITCH, I2);
    F7 = FLOAT R0;
    R1 = 0x46fe0000;                      // 32512.0 = note 127
    F7 = MIN(F7, F1);
    R1 = 0x39aaaaab;                      // 1 / (256 * 12)
    F0 = F7 * F1;
    CALL _dv_exp2;
    R1 = 0x39329a62;                      // 440 * 2^(-69/12) / 48000
    F0 = F0 * F1;
    F7 = F0;

    // ---- level from velocity ----
    R0 = DM(DK_IN_W_VELOCITY, I2);
    F0 = FLOAT R0;
    R1 = 0x38010204;                      // 1 / 32512
    F0 = F0 * F1;
    R1 = 0x3eb33333;                      // 0.35
    F9 = F0 * F1;

    // ---- BEND, INDEX, RATIO ----
    R1 = 0x38010204;
    R0 = DM(0, I1);
    F0 = FLOAT R0;
    F6 = F0 * F1;                         // BEND
    R0 = DM(1, I1);
    F0 = FLOAT R0;
    F5 = F0 * F1;                         // INDEX
    R0 = DM(2, I1);
    F0 = FLOAT R0;
    F0 = F0 * F1;                         // RATIO 0 .. 1
    R1 = 0x40700000;                      // 3.75
    NOP;
    F0 = F0 * F1;
    R1 = 0x3e800000;                      // 0.25
    NOP;
    F10 = F0 + F1;                        // ratio multiplier 0.25 .. 4

    // ---- envelopes, from the block counter (zeroed at note-on) ----
    R11 = 0x3f800000;                     // 1.0
    R0 = DM(1, I3);
    R1 = R0 + 1;
    DM(1, I3) = R1;
    F0 = FLOAT R0;                        // n

    // carrier pitch: env = (1 - n/40)^2, inc = base * (1 + 2.5 BEND env)
    R1 = 0x3ccccccd;                      // 0.025 = 1/40
    NOP;
    F2 = F0 * F1;
    R1 = 0x3f800000;                      // 1.0
    NOP;
    F2 = F11 - F2;
    R1 = 0;                               // 0.0
    NOP;
    F2 = MAX(F2, F1);
    NOP;
    F2 = F2 * F2;                         // squared: quickest at the strike
    F1 = F2 * F6;                         // BEND * env
    NOP;
    F1 = F1 + F1;                         // x2
    F3 = F1 * F4;                         // x0.5
    NOP;
    F1 = F1 + F3;                         // x2.5
    F1 = F1 + F11;                        // 1 + 2.5 BEND env
    NOP;
    F12 = F7 * F1;                        // carrier increment
    F14 = F12 * F10;                      // modulator increment, by ratio

    // index: I = 8 INDEX (1 - n/20)^2
    R1 = 0x3c124925;                      // 0.0089 = 1/112: the index lasts 112 blocks, about 160 ms. At 1/20 it was
    NOP;                                  // gone in 29 ms, so every sideband sat inside the transient and the body the
    F2 = F0 * F1;                         // ear hears afterwards was an unmodulated sine -- heard as 'a click'
    R1 = 0x3f800000;                      // 1.0
    NOP;
    F2 = F11 - F2;
    R1 = 0;                               // 0.0
    NOP;
    F2 = MAX(F2, F1);
    NOP;
    F2 = F2 * F2;
    F2 = F2 * F5;                         // x INDEX
    R1 = 0x40400000;                      // 3.0: two cycles of excursion, which is real FM. Now that the phase is
    NOP;                                  // wrapped before the sine, the polynomial is valid there; before the wrap
    F2 = F2 * F1;                         // this had to be half a cycle and the modulation was barely audible
    R1 = F2;
    DM(3, I3) = R1;                       // the index for this block

    // noise envelope: (1 - n/60), about 87 ms, so NOISE turns the drum into a snare or a hat
    R1 = 0x3d088889;                      // 1/60
    NOP;
    F2 = F0 * F1;
    R1 = 0x3f800000;                      // 1.0
    NOP;
    F2 = F11 - F2;
    R1 = 0;                               // 0.0
    NOP;
    F2 = MAX(F2, F1);
    R1 = F2;
    DM(6, I3) = R1;
    R1 = 0x38010204;                      // 1 / 32512
    R0 = DM(3, I1);
    F0 = FLOAT R0;
    F0 = F0 * F1;                         // NOISE 0 .. 1
    R1 = F0;
    DM(7, I3) = R1;

    // ---- constants and state ----
    R0 = DM(DK_IN_W_OUT, I2);
    I0 = R0;
    R13 = DM(0, I3);                      // carrier phase
    R15 = DM(2, I3);                      // modulator phase
    R4 = 0x3f000000;                      // 0.5
    R8 = 0x3e800000;                      // 0.25
    LCNTR = DK_BLOCK, DO _bd_last UNTIL LCE;

    // ---- modulator: sin(2 pi modulator phase) ----
    F0 = F15 - F4;
    NOP;
    F2 = CLIP F0 BY F8;
    F2 = F2 + F2;
    F0 = F2 - F0;
    NOP;
    F1 = F0 * F0;
    R2 = 0x42283c1a;                      // a9
    F2 = F2 * F1;
    R3 = 0xc2996966;                      // a7
    F2 = F2 + F3;
    F2 = F2 * F1;
    R3 = 0x42a335e3;                      // a5
    F2 = F2 + F3;
    F2 = F2 * F1;
    R3 = 0xc2255de7;                      // a3
    F2 = F2 + F3;
    F2 = F2 * F1;
    R3 = 0x40c90fdb;                      // a1
    F2 = F2 + F3;
    F2 = F2 * F0;
    F2 = -F2;
    R0 = F2;
    NOP;
    DM(4, I3) = R0;                       // the modulator value

    // ---- carrier: sin(2 pi (carrier phase + I * modulator)) ----
    R0 = DM(3, I3);
    F0 = PASS R0;                         // I. The modulator goes through R2: R0 IS F0, so reading it into R0 a
    NOP;                                  // second time would overwrite the index and the multiply becomes m x m
    R2 = DM(4, I3);
    F2 = PASS R2;                         // modulator
    NOP;
    F0 = F0 * F2;                         // I m
    NOP;
    F0 = F0 + F13;                        // the modulated phase
    NOP;
    // wrap it into [0, 1) before the sine. The excursion is at most about a cycle either way, so a couple of
    // conditional subtractions are enough -- and without this the polynomial reads *outside* its range and what comes
    // out is a folded wave, which is why INDEX only brightened and RATIO only clicked: no real sidebands at all
    F1 = F0 - F11;                        // -1
    IF GE F0 = PASS F1;
    F1 = F0 - F11;
    IF GE F0 = PASS F1;
    F1 = F0 + F11;                        // +1
    IF LT F0 = PASS F1;
    NOP;
    F0 = F0 - F4;
    NOP;
    F2 = CLIP F0 BY F8;
    F2 = F2 + F2;
    F0 = F2 - F0;
    NOP;
    F1 = F0 * F0;
    R2 = 0x42283c1a;
    F2 = F2 * F1;
    R3 = 0xc2996966;
    F2 = F2 + F3;
    F2 = F2 * F1;
    R3 = 0x42a335e3;
    F2 = F2 + F3;
    F2 = F2 * F1;
    R3 = 0xc2255de7;
    F2 = F2 + F3;
    F2 = F2 * F1;
    R3 = 0x40c90fdb;
    F2 = F2 + F3;
    F2 = F2 * F0;
    F2 = -F2;                             // the output

    // ---- noise layer: xorshift, long envelope ----
    R0 = DM(5, I3);
    R1 = LSHIFT R0 BY 13;
    R0 = R0 XOR R1;
    R1 = -17;
    R1 = ASHIFT R0 BY R1;
    R0 = R0 XOR R1;
    R1 = LSHIFT R0 BY 5;
    R0 = R0 XOR R1;
    DM(5, I3) = R0;
    F3 = FLOAT R0;
    R1 = 0x30000000;                      // 2^-31
    F1 = PASS R1;
    NOP;
    F3 = F3 * F1;                         // noise -1 .. 1
    NOP;
    R1 = DM(6, I3);
    F1 = PASS R1;                         // envelope
    NOP;
    F3 = F3 * F1;
    NOP;
    R1 = DM(7, I3);
    F1 = PASS R1;                         // NOISE
    NOP;
    F3 = F3 * F1;
    NOP;
    F2 = F2 + F3;

    // ---- level and bound ----
    F2 = F2 * F9;
    NOP;
    R0 = 0x3f800000;                      // +1
    NOP;
    F3 = MIN(F2, F0);
    R1 = 0xbf800000;                      // -1
    NOP;
    F2 = MAX(F3, F1);
    DM(0, I0) = F2;

    // ---- advance ----
    F13 = F13 + F12;
    F0 = F13 - F11;
    IF GE F13 = PASS F0;
    F15 = F15 + F14;
    F0 = F15 - F11;
    IF GE F15 = PASS F0;
_bd_last: MODIFY(I0, 4);

    DM(0, I3) = R13;
    DM(2, I3) = R15;
    R0 = DK_OK;
    JUMP (M13, I13);

_bd_note_on:
    I3 = R4;
    R0 = 0x2545f491;                      // seed the noise: a zero xorshift state stays zero
    DM(5, I3) = R0;
    R0 = 0;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
.GLOBAL _bd_desc;
.VAR _bd_desc = DK_MAGIC;
.VAR _bd_desc1 = DK_MACHINE_V1;
.VAR _bd_desc2 = 3;
.VAR _bd_desc3 = DK_MAX_VOICES;
.VAR _bd_desc4 = 32;                      // carrier phase, counter, modulator phase, index, modulator value
.VAR _bd_desc5 = 12;                      // phase, counter, modulator phase zeroed at note-on
.VAR _bd_desc6 = 1200;
.VAR _bd_desc7 = 200;
.VAR _bd_desc8 = 2;
.VAR _bd_desc9 = 0;
