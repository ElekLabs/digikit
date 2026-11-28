// Bode-style barber-pole phaser. A swept single-sideband copy of an all-pass
// cascade interferes with the direct track in the send return mix. The phase
// slope moves the cancellation frequencies in one direction and wraps them.
// Topology: Esqueda et al., "Barberpole Phasing and Flanging Illusions", DAFx-15,
// section 4.1. Stereo channels share phase and ramps and have separate histories.

#include "dk_fx.inc"
#include "ascend.h"

#define ASC_PHASE 0
#define ASC_INC 1
#define ASC_DEPTH 2
#define ASC_COEF 3
#define ASC_DEPTH_START 4
#define ASC_DEPTH_STEP 5
#define ASC_COEF_START 6
#define ASC_COEF_STEP 7
#define ASC_INPUT 8
#define ASC_OUTPUT 9

// Byte offsets from the state pointer. Each channel's 21 words contain the
// four phaser stages, eight P states, eight Q states and the delayed P sample.
// The channels reuse the 32-sample all-pass and 33-word delayed P buffers.
#define ASC_LEFT_B 40
#define ASC_CHANNEL_BYTES 84
#define ASC_P_B 16
#define ASC_Q_B 48
#define ASC_CARRY 20
#define ASC_PASS_B 208
#define ASC_DELAY_B 336
#define ASC_PHASE_PAIRS_B 468
#define ASC_STATE_BYTES 724

.SECTION/PM seg_pmco;
.GLOBAL _ascend_process;

_ascend_process:
    I5 = R4;
    I0 = R8;
    I1 = R12;
    R0 = DM(DK_FX_IN_W_IN, I0);
    DM(ASC_INPUT, I5) = R0;
    R0 = DM(DK_FX_IN_W_OUT, I0);
    DM(ASC_OUTPUT, I5) = R0;

    R0 = DM(ASCEND_RATE_WORD, I1);
    R1 = 0x3d23d70a;                     // 0.04 Hz
    F0 = MAX(F0, F1);
    R1 = 0x3e99999a;                     // 0.30 Hz
    F0 = MIN(F0, F1);
    R1 = 0x47aec33e;                     // 2^32 / 48000
    F0 = F0 * F1;
    R2 = DM(ASCEND_DIRECTION_WORD, I1);
    R1 = 0x3c820821;                     // 1/63
    F2 = F2 * F1;
    R1 = 0x3f800000;
    F2 = CLIP F2 BY F1;                  // -1 .. 1, zero holds phase
    F0 = F0 * F2;
    R3 = TRUNC F0;                       // signed target increment
    R8 = DM(ASC_INC, I5);
    DM(ASC_INC, I5) = R3;
    R0 = R3 - R8;
    R1 = -5;
    R10 = ASHIFT R0 BY R1;
    R9 = DM(ASC_PHASE, I5);

    // Interpolate coefficient and the shifted copy's gain over the block.
    R0 = DM(ASCEND_DENSITY_WORD, I1);
    R1 = 0;
    F0 = MAX(F0, F1);
    R1 = 0x3f800000;
    F0 = MIN(F0, F1);
    R1 = 0x3eb33333;                     // c = 0.30 + 0.35 * density
    F0 = F0 * F1;
    R1 = 0x3e99999a;
    F0 = F0 + F1;
    R2 = DM(ASC_COEF, I5);
    DM(ASC_COEF, I5) = R0;
    DM(ASC_COEF_START, I5) = R2;
    F3 = F0 - F2;
    R1 = 0x3d000000;                     // 1/32
    F3 = F3 * F1;
    DM(ASC_COEF_STEP, I5) = R3;

    R0 = DM(ASCEND_DEPTH_WORD, I1);
    R1 = 0;
    F0 = MAX(F0, F1);
    R1 = 0x3f800000;
    F0 = MIN(F0, F1);
    R1 = 0x3f000000;                     // g = 0.5 * depth
    F0 = F0 * F1;
    R2 = DM(ASC_DEPTH, I5);
    DM(ASC_DEPTH, I5) = R0;
    DM(ASC_DEPTH_START, I5) = R2;
    F3 = F0 - F2;
    R1 = 0x3d000000;
    F3 = F3 * F1;
    DM(ASC_DEPTH_STEP, I5) = R3;

    R0 = I5;
    R1 = ASC_PHASE_PAIRS_B;
    R2 = R0 + R1;
    I4 = R2;
    CALL _asc_make_osc;
    DM(ASC_PHASE, I5) = R9;

    R0 = I5;
    R1 = ASC_LEFT_B;
    R2 = R0 + R1;
    I12 = R2;
    CALL _asc_channel;
    R0 = I12;
    R1 = ASC_CHANNEL_BYTES;
    R2 = R0 + R1;
    I12 = R2;
    CALL _asc_channel;

    R0 = DK_FX_OK;
    JUMP (M13, I13);

// Process a channel at I12. I5 is the effect state, shared by both channels.
_asc_channel:
    I0 = DM(ASC_INPUT, I5);
    R0 = I5;
    R1 = ASC_PASS_B;
    R2 = R0 + R1;
    I1 = R2;
    I3 = I12;
    CALL _asc_allpass;

    R0 = I5;
    R1 = ASC_PASS_B;
    R2 = R0 + R1;
    I0 = R2;
    R1 = ASC_DELAY_B;
    R2 = R0 + R1;
    I1 = R2;
    I3 = I12;
    R3 = DM(ASC_CARRY, I3);
    DM(I1, M6) = R3;
    R0 = I12;
    R1 = ASC_P_B;
    R2 = R0 + R1;
    I3 = R2;
    CALL _asc_pass_p;

    R0 = I5;
    R1 = ASC_PASS_B;
    R2 = R0 + R1;
    I0 = R2;
    I1 = DM(ASC_OUTPUT, I5);
    R1 = ASC_DELAY_B;
    R2 = R0 + R1;
    I2 = R2;
    R1 = ASC_PHASE_PAIRS_B;
    R2 = R0 + R1;
    I4 = R2;
    R0 = I12;
    R1 = ASC_Q_B;
    R2 = R0 + R1;
    I3 = R2;
    CALL _asc_pass_q;
    R0 = DM(I2, M5);
    I3 = I12;
    DM(ASC_CARRY, I3) = R0;

    // Return only the signed correction. At full depth and unity send and MIX,
    // the return mix adds this to the direct track, leaving equal gains on the
    // direct and shifted paths. Depth zero leaves the dry track untouched.
    I0 = DM(ASC_INPUT, I5);
    I1 = DM(ASC_OUTPUT, I5);
    R2 = DM(ASC_DEPTH_START, I5);
    R3 = DM(ASC_DEPTH_STEP, I5);
    R4 = 0x40fccccd;                     // 7.9, just under the guard at 8
    F5 = -F4;
    LCNTR = 32, DO _asc_mix UNTIL LCE;
        R0 = DM(I0, M6);                 // x
        R1 = DM(I1, M5);                 // shifted
        F2 = F2 + F3;
        F1 = F1 - F0;
        F1 = F1 * F2;
        F1 = MIN(F1, F4);
        F1 = MAX(F1, F5);
_asc_mix: DM(I1, M6) = R1;
    R0 = I0;
    DM(ASC_INPUT, I5) = R0;
    R0 = I1;
    DM(ASC_OUTPUT, I5) = R0;
    RTS;

// Four first-order sections with transfer (c - z^-1) / (1 - c*z^-1).
// The cascade concentrates more phase change at lower frequencies as c rises.
_asc_allpass:
    R4 = DM(0, I3);
    R5 = DM(1, I3);
    R6 = DM(2, I3);
    R7 = DM(3, I3);
    R8 = DM(ASC_COEF_START, I5);
    R9 = DM(ASC_COEF_STEP, I5);
    LCNTR = 32, DO _asc_ap UNTIL LCE;
        R0 = DM(I0, M6);
        F8 = F8 + F9;
        F2 = F8 * F0;
        F1 = F2 + F4;
        F2 = F8 * F1;
        F4 = F2 - F0;
        F2 = F8 * F1;
        F0 = F2 + F5;
        F2 = F8 * F0;
        F5 = F2 - F1;
        F2 = F8 * F0;
        F1 = F2 + F6;
        F2 = F8 * F1;
        F6 = F2 - F0;
        F2 = F8 * F1;
        F0 = F2 + F7;
        F2 = F8 * F0;
        F7 = F2 - F1;
_asc_ap: DM(I1, M6) = R0;
    DM(0, I3) = R4;
    DM(1, I3) = R5;
    DM(2, I3) = R6;
    DM(3, I3) = R7;
    RTS;

#include "quadrature.inc"

.SECTION/DM seg_dmda;
.GLOBAL _ascend_desc;
.VAR _ascend_desc = DK_FX_MAGIC;
.VAR _ascend_desc1 = DK_FX_V1;
.VAR _ascend_desc2 = ASCEND_KNOBS;
.VAR _ascend_desc3 = ASC_STATE_BYTES;
.VAR _ascend_desc4 = 2;
.VAR _ascend_desc5 = 0;
