// machines/wtstack: the track's sample as a Serum wavetable, up to four voices, as a stereo voice of the SDK contract
// (sdk/dk_machine.h; [[voice]] stereo: 32 left samples at out, then 32 right ones). tools/patchkit/wtstack_ref.py is the model.
//
// Knobs (params): 0 WAVE, 1 DTUN, 2-4 INT2-INT4, 5 SPRD (knob H; the row's `lev` gives the track a fixed level).
//   table  the even voice's record: word 0 the sample's first frame (a byte address; 0 without a sample), +0x188 its frames.
//          Frames of FRAME samples (Serum's 2048), as many as fit; a shorter sample is one frame of its own length.
//   WAVE   the position over the frames: two neighbouring frames, each read with linear interpolation, crossfaded
//   voices voice 1 plays the note; voice k (2-4) plays it INTk semitones away: step 64 (0x4000) is OFF, 65 the note itself
//          (UNI), below 64 -24 .. -1, above 65 +1 .. +24. The active voices, in order, alternate +, -, +, - for the detune and
//          left, right, left, right for the spread; the third and fourth get half of both, so four in unison are four voices.
//   DTUN   +/- d^3 semitones, d = (word - 0x4000) / 0x3f00 (1 at full, so 100 cents; 64 steps below that is 1/4 cent)
//   SPRD   the pan of an active voice: the centre +/- SPRD (equal power); a single voice stays at the centre
//   out    2 / sqrt(n) per voice for n active voices
//
// The sample is 16-bit. The assembler drops a load's (SWSE) and misencodes the immediate ASHIFT and FEXT (SE), so the loop
// reads the aligned word that holds a sample, shifts the sample to the upper half and masks the lower one: the sample x 2^16.
//
// State (96 bytes; the phases, 16 bytes, are cleared at the note-on): words S_* below, per voice k at S_x + k.
// Render loop registers: F9 the phase, R6 / R7 the aligned words of frames A and B, R11 the upper half's mask, F13 / R14 the frame's length, R15 1 when
// the sample starts at the upper half of a word; I1 the voice's state, I3 the state, I0 left, I5 right, I4 the reads.

#include "dk_machine.inc"
#include "tables.inc"

#define S_PH 0
#define S_FR 4
#define S_INC 5
#define S_GL 9
#define S_GR 13
#define S_ON 17
#define REC_FRAMES 98

.SECTION/PM seg_pmco;
.GLOBAL _ws_render;
.GLOBAL _ws_note_on;
.EXTERN _dv_exp2;
.EXTERN _dv_g;
.EXTERN _ws_k;
.EXTERN _ws_pan;

_ws_render:
    I3 = R4;
    I2 = R8;
    I5 = R12;
    R0 = _ws_k;
    I1 = R0;

    R0 = DM(DK_IN_W_OUT, I2);
    I0 = R0;
    R1 = 0;
    LCNTR = 64, DO _ws_z UNTIL LCE;
_ws_z: DM(I0, M6) = R1;

    // the table
    R0 = DM(_dv_g + 16);                  // the even voice's record
    I4 = R0;
    R15 = DM(0, I4);
    R14 = DM(REC_FRAMES, I4);
    R15 = PASS R15;
    IF EQ JUMP _ws_out;
    R0 = 4;
    COMP(R14, R0);
    IF LT JUMP _ws_out;
    R1 = 1;                               // frames in the table
    R0 = FRAME;
    COMP(R14, R0);
    IF LT JUMP _ws_one;
    R1 = LSHIFT R14 BY -11;
    R14 = PASS R0;
_ws_one:
    R0 = FEXT R15 BY 1:1;
    R2 = -4;
    R13 = R15 AND R2;                     // the aligned base
    R15 = PASS R0;

    // WAVE: frames f0 (A) and f1 (B), the fraction between them
    R0 = DM(0, I5);
    F0 = FLOAT R0;
    R2 = DM(K_W01, I1);
    F0 = F0 * F2;
    R2 = DM(K_ONE, I1);
    F0 = MIN(F0, F2);
    R1 = R1 - 1;
    F2 = FLOAT R1;
    F0 = F0 * F2;
    R3 = TRUNC F0;
    F2 = FLOAT R3;
    F0 = F0 - F2;
    DM(S_FR, I3) = R0;
    R2 = R3 + 1;
    R2 = MIN(R2, R1);
    R0 = LSHIFT R14 BY 1;                 // bytes per frame (even when there are two or more)
    R6 = R3 * R0 (SSI);
    R6 = R6 + R13;
    R7 = R2 * R0 (SSI);
    R7 = R7 + R13;
    F13 = FLOAT R14;

    // the active voices
    R0 = 0x4000;
    R4 = 1;
    R8 = 1;
    DM(S_ON, I3) = R4;
    R2 = DM(2, I5);
    R1 = 0;
    COMP(R2, R0);
    IF NE R1 = PASS R4;
    DM(S_ON + 1, I3) = R1;
    R8 = R8 + R1;
    R2 = DM(3, I5);
    R1 = 0;
    COMP(R2, R0);
    IF NE R1 = PASS R4;
    DM(S_ON + 2, I3) = R1;
    R8 = R8 + R1;
    R2 = DM(4, I5);
    R1 = 0;
    COMP(R2, R0);
    IF NE R1 = PASS R4;
    DM(S_ON + 3, I3) = R1;
    R8 = R8 + R1;

    // DTUN: F11 = d^3 DMAX pitch words
    R0 = DM(1, I5);
    R1 = 0x4000;
    R0 = R0 - R1;
    F0 = FLOAT R0;
    R1 = DM(K_DTN, I1);
    F0 = F0 * F1;
    R1 = DM(K_ONE, I1);
    F0 = MIN(F0, F1);
    R1 = DM(K_M1, I1);
    F0 = MAX(F0, F1);
    F1 = F0 * F0;
    F0 = F0 * F1;
    R1 = DM(K_DMAX, I1);
    F11 = F0 * F1;

    // SPRD: R12 = pan steps either side of the centre (0 for a single voice)
    R0 = DM(5, I5);
    F0 = FLOAT R0;
    R1 = DM(K_SPR, I1);
    F0 = F0 * F1;
    R1 = DM(K_HALF, I1);
    F0 = F0 + F1;
    R12 = TRUNC F0;
    R0 = 1;
    COMP(R8, R0);
    IF EQ R12 = R12 - R12;

    // F10 = the gain of each of n voices
    R0 = LSHIFT R8 BY 2;
    R1 = I1;
    R0 = R0 + R1;
    I4 = R0;
    R10 = DM(K_G0, I4);

    // per voice k (R4), j (R5) = the active voices before it: the increment and the two gains
    R4 = 0;
    R5 = 0;
_ws_vk:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I0 = R0;
    R0 = DM(S_ON, I0);
    R0 = PASS R0;
    IF EQ JUMP _ws_vnext;
    R0 = FEXT R5 BY 0:1;
    R1 = DM(K_ONE, I1);
    R2 = DM(K_M1, I1);
    R0 = PASS R0;
    IF NE R1 = PASS R2;
    F9 = PASS F1;                         // the voice's side: +1 or -1
    R0 = FEXT R5 BY 1:1;
    R0 = PASS R0;
    IF EQ JUMP _ws_p0;
    R1 = DM(K_HALF, I1);
    F9 = F9 * F1;                         // the second pair: half the detune
_ws_p0:
    R1 = 0;
    R2 = PASS R4;
    IF EQ JUMP _ws_k0;
    R2 = LSHIFT R4 BY 2;
    R3 = I5;
    R2 = R2 + R3;
    I4 = R2;
    R1 = DM(1, I4);
    R2 = 0x4000;
    R1 = R1 - R2;
    R2 = 0x100;
    R3 = R1 - R2;
    R1 = PASS R1;
    IF GT R1 = PASS R3;                   // the interval in pitch words: UNI is 0
_ws_k0:
    R0 = DM(DK_IN_W_PITCH, I2);
    R0 = R0 + R1;
    F0 = FLOAT R0;
    F1 = F11 * F9;
    F0 = F0 + F1;
    R1 = DM(K_SEMI, I1);
    F0 = F0 * F1;
    R1 = DM(K_OCTMAX, I1);
    F0 = MIN(F0, F1);
    R1 = 0;
    F0 = MAX(F0, F1);
    CALL _dv_exp2;
    R1 = DM(K_NOTE0, I1);
    F0 = F0 * F1;
    R1 = DM(K_INVFS, I1);
    F0 = F0 * F1;
    F0 = F0 * F13;                        // table samples per output sample
    DM(S_INC, I0) = R0;
    R1 = PASS R12;
    R3 = LSHIFT R12 BY -1;
    R0 = FEXT R5 BY 1:1;
    R0 = PASS R0;
    IF NE R1 = PASS R3;                   // the second pair: half the spread
    R0 = FEXT R5 BY 0:1;
    R0 = PASS R0;
    IF NE R1 = -R1;
    R2 = 128;
    R1 = R1 + R2;                         // the pan step: 0 hard left .. 256 hard right
    R3 = _ws_pan;
    R0 = LSHIFT R1 BY 2;
    R0 = R0 + R3;
    I4 = R0;
    R0 = DM(0, I4);
    F0 = F0 * F10;
    DM(S_GL, I0) = R0;
    R2 = PAN_N;
    R1 = R2 - R1;
    R0 = LSHIFT R1 BY 2;
    R0 = R0 + R3;
    I4 = R0;
    R0 = DM(0, I4);
    F0 = F0 * F10;
    DM(S_GR, I0) = R0;
    R5 = R5 + 1;
_ws_vnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _ws_vk;

    // render each active voice into both channels
    R12 = 0;
_ws_rk:
    R0 = LSHIFT R12 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I1 = R0;
    R0 = DM(S_ON, I1);
    R0 = PASS R0;
    IF EQ JUMP _ws_rnext;
    R9 = DM(S_PH, I1);
    COMP(F9, F13);
    IF GE R9 = R9 - R9;                   // a shorter table since the last block
    R0 = DM(DK_IN_W_OUT, I2);
    I0 = R0;
    R1 = 0x80;
    R0 = R0 + R1;
    I5 = R0;
    R11 = 0xffff0000;
    LCNTR = DK_BLOCK, DO _ws_s UNTIL LCE;
    R0 = TRUNC F9;
    F1 = FLOAT R0;
    F1 = F9 - F1;                         // the fraction
    R2 = R0 + 1;
    COMP(R2, R14);
    IF GE R2 = R2 - R2;                   // the next sample, the frame's first past its end
    R0 = R0 + R15;
    R2 = R2 + R15;
    R3 = FEXT R0 BY 0:1;
    R3 = BTGL R3 BY 0;
    R3 = LSHIFT R3 BY 4;                  // the shift that puts the sample in the upper half
    R4 = FEXT R2 BY 0:1;
    R4 = BTGL R4 BY 0;
    R4 = LSHIFT R4 BY 4;
    R0 = LSHIFT R0 BY -1;
    R0 = LSHIFT R0 BY 2;
    R2 = LSHIFT R2 BY -1;
    R2 = LSHIFT R2 BY 2;
    R5 = R6 + R0;
    I4 = R5;
    R5 = DM(0, I4);
    R5 = LSHIFT R5 BY R3;
    R5 = R5 AND R11;
    F5 = FLOAT R5;
    R8 = R6 + R2;
    I4 = R8;
    R8 = DM(0, I4);
    R8 = LSHIFT R8 BY R4;
    R8 = R8 AND R11;
    F8 = FLOAT R8;
    F8 = F8 - F5;
    F8 = F8 * F1;
    F5 = F5 + F8;                         // frame A
    R10 = R7 + R0;
    I4 = R10;
    R10 = DM(0, I4);
    R10 = LSHIFT R10 BY R3;
    R10 = R10 AND R11;
    F10 = FLOAT R10;
    R8 = R7 + R2;
    I4 = R8;
    R8 = DM(0, I4);
    R8 = LSHIFT R8 BY R4;
    R8 = R8 AND R11;
    F8 = FLOAT R8;
    F8 = F8 - F10;
    F8 = F8 * F1;
    F10 = F10 + F8;                       // frame B
    F10 = F10 - F5;
    R8 = DM(S_FR, I3);
    F10 = F10 * F8;
    F5 = F5 + F10;
    R8 = DM(S_GL, I1);
    F8 = F5 * F8;
    R0 = DM(0, I0);
    F0 = F0 + F8;
    DM(I0, M6) = R0;
    R8 = DM(S_GR, I1);
    F8 = F5 * F8;
    R0 = DM(0, I5);
    F0 = F0 + F8;
    DM(I5, M6) = R0;
    R8 = DM(S_INC, I1);
    F9 = F9 + F8;
    COMP(F9, F13);
_ws_s: IF GE F9 = F9 - F13;
    DM(S_PH, I1) = R9;
_ws_rnext:
    R12 = R12 + 1;
    R0 = 4;
    COMP(R12, R0);
    IF LT JUMP _ws_rk;

_ws_out:
    R0 = DK_OK;
    JUMP (M13, I13);

_ws_note_on:
    R0 = 0;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
// magic, contract, n_params, voices, state_bytes, state_clear_bytes, max_cycles_block, max_cycles_note_on,
// max_call_depth, stack_bytes
.GLOBAL _ws_desc;
.VAR _ws_desc = DK_MAGIC;
.VAR _ws_desc1 = DK_MACHINE_V1;
.VAR _ws_desc2 = 6;
.VAR _ws_desc3 = DK_MAX_VOICES;
.VAR _ws_desc4 = 96;
.VAR _ws_desc5 = 16;
.VAR _ws_desc6 = 10500;
.VAR _ws_desc7 = 50;
.VAR _ws_desc8 = 2;
.VAR _ws_desc9 = 0;
