// machines/para: the track's sample as a Serum wavetable played paraphonically: each trig takes the next of four voices, with
// its own note and amp envelope; a stereo voice of the SDK contract (sdk/dk_machine.h; [[voice]] stereo: 32 left samples at
// out, then 32 right ones). tools/patchkit/para_ref.py is the model, operation for operation.
//
// Knobs (params): 0 WAVE, 1 DTUN, 2 ATK, 3 DEC, 4 JUMP, 5 SPRD (knob H; the row's `lev` gives the track a fixed level).
//   table  the even voice's record, word 0 the sample's first frame (a byte address), +0x188 its frames; frames
//          of FRAME samples, a shorter sample is one frame
//   WAVE   the position over the frames: two neighbouring frames, each read with linear interpolation, crossfaded
//   JUMP   voice slot k plays WAVE + k x JUMP frames (JUMP = (word >> 8) - 64), wrapped around the table
//   voices note_on gives the next slot (0, 1, 2, 3, 0, ...) the note, phase 0 and the attack; the envelope rises by ATK's step
//          per sample from where it is to 1, holds until the track's note-off (the release latch, byte v = 2t), then falls by
//          DEC's factor per sample and stops below -80 dB
//   DTUN   slot k at +/- d^3 semitones (+, -, +1/2, -1/2), d = (word - 0x4000) / 0x3f00
//   SPRD   slot k at the centre +/- SPRD (right, left, right/2, left/2), equal power
//   out    the voices' sum, each at half a single full-scale voice
// render returns DK_DONE when every slot has stopped.
//
// The sample is 16-bit; the loop reads the aligned word that holds a sample, shifts the sample to the upper half
// and masks the lower one (the assembler drops a load's (SWSE)).
//
// State (512 bytes, a power of two for the track index; loaded as zeros, never cleared): words S_* below, per slot k at S_x + k.
// Render loop registers: F9 the phase, R6 / R7 frames A and B, R11 the upper half's mask, F13 / R14 the frame's length, R15 1
// when the sample starts at the upper half of a word; I1 the slot's state, I2 the envelope, I3 the state, I0 left, I5 right,
// I4 the reads.

#include "dk_machine.inc"
#include "tables.inc"

#define S_PH 0
#define S_LVL 4
#define S_STG 8
#define S_PIT 12
#define S_INC 16
#define S_GL 20
#define S_GR 24
#define S_FA 28
#define S_FB 32
#define S_NEXT 36
#define S_FR 37
#define S_W 38
#define S_ODD 39
#define S_NF 40
#define S_OUT 41
#define S_AI 42
#define S_DC 43
#define S_OFF 44
#define S_ENV 48
#define REC_FRAMES 98

.SECTION/PM seg_pmco;
.GLOBAL _pa_render;
.GLOBAL _pa_note_on;
.EXTERN _dv_exp2;
.EXTERN _dv_g;
.EXTERN _fw_frame;
.EXTERN _pa_k;
.EXTERN _pa_pan;
.EXTERN _pa_atk;
.EXTERN _pa_dec;
.EXTERN _dkstate_para;
.EXTERN _fw_release_latch;

_pa_render:
    I3 = R4;
    I2 = R8;
    I5 = R12;
    R0 = _pa_k;
    I1 = R0;

    R0 = DM(DK_IN_W_OUT, I2);
    DM(S_OUT, I3) = R0;
    I0 = R0;
    R1 = 0;
    LCNTR = 64, DO _pa_z UNTIL LCE;
_pa_z: DM(I0, M6) = R1;

    // the track's note-off (the release latch byte v = 2t): attacking and held slots decay
    R0 = _dkstate_para;
    R1 = I3;
    R2 = R1 - R0;
    R0 = FEXT R2 BY 9:4;                  // t
    R0 = LSHIFT R0 BY 1;                  // v
    R1 = 0xfffffffc;
    R2 = R0 AND R1;
    R3 = _fw_release_latch;
    R1 = R2 + R3;
    I4 = R1;
    R1 = DM(0, I4);
    R2 = 3;
    R3 = R0 AND R2;
    R2 = LSHIFT R3 BY 3;
    R3 = -R2;
    R2 = LSHIFT R1 BY R3;
    R1 = 0xff;
    R0 = R2 AND R1;
    R0 = PASS R0;
    IF EQ JUMP _pa_held;
    R4 = 0;
_pa_rel:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I4 = R0;
    R0 = DM(S_STG, I4);
    R1 = 1;
    COMP(R0, R1);
    IF EQ JUMP _pa_rset;
    R1 = 2;
    COMP(R0, R1);
    IF NE JUMP _pa_relnext;
_pa_rset:
    R1 = 3;
    DM(S_STG, I4) = R1;                   // ATTACK or HELD: DECAY
_pa_relnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _pa_rel;
_pa_held:

    // the table
    R0 = DM(_dv_g + 16);                  // the even voice's record
    I4 = R0;
    R15 = DM(0, I4);
    R14 = DM(REC_FRAMES, I4);
    R15 = PASS R15;
    IF EQ JUMP _pa_out;
    R0 = 4;
    COMP(R14, R0);
    IF LT JUMP _pa_out;
    R1 = 1;                               // frames in the table
    R0 = FRAME;
    COMP(R14, R0);
    IF LT JUMP _pa_one;
    R1 = LSHIFT R14 BY -11;
    R14 = PASS R0;
_pa_one:
    DM(S_NF, I3) = R1;
    DM(S_W, I3) = R14;
    R0 = FEXT R15 BY 1:1;
    DM(S_ODD, I3) = R0;
    R2 = -4;
    R13 = R15 AND R2;                     // the aligned base

    // WAVE: frame f0 (R12) and the fraction
    R0 = DM(0, I5);
    F0 = FLOAT R0;
    R2 = DM(K_W01, I1);
    F0 = F0 * F2;
    R2 = DM(K_ONE, I1);
    F0 = MIN(F0, F2);
    R1 = R1 - 1;
    F2 = FLOAT R1;
    F0 = F0 * F2;
    R12 = TRUNC F0;
    F2 = FLOAT R12;
    F0 = F0 - F2;
    // smoothing off (bit 0 of the track's core/trackflags word, frame byte 0xf8 + 0x60 t): the nearer frame alone
    R2 = DM(_dv_g + 12);
    R4 = 0x60;
    R2 = R2 * R4 (SSI);
    R4 = _fw_frame;
    R2 = R2 + R4;
    I4 = R2;
    R2 = DM(62, I4);
    R2 = FEXT R2 BY 0:1;
    R2 = PASS R2;
    IF EQ JUMP _pa_smooth;
    R4 = DM(K_ONE, I1);
    R2 = DM(K_HALF, I1);
    COMP(F0, F2);
    R0 = 0;
    IF GE R0 = PASS R4;
_pa_smooth:
    DM(S_FR, I3) = R0;
    // JUMP: R10 frames per slot
    R10 = DM(4, I5);
    R10 = LSHIFT R10 BY -8;
    R0 = 64;
    R10 = R10 - R0;

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

    // SPRD: R8 = pan steps either side of the centre
    R0 = DM(5, I5);
    F0 = FLOAT R0;
    R1 = DM(K_SPR, I1);
    F0 = F0 * F1;
    R1 = DM(K_HALF, I1);
    F0 = F0 + F1;
    R8 = TRUNC F0;

    // per sounding slot k (R4): its frames, its increment and its two gains
    R4 = 0;
_pa_vk:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I0 = R0;
    R0 = DM(S_STG, I0);
    R0 = PASS R0;
    IF EQ JUMP _pa_vnext;
    // frame A = f0 + k JUMP, wrapped; frame B the next, at most the last
    R5 = DM(S_NF, I3);
    R0 = R4 * R10 (SSI);
    R0 = R0 + R12;
_pa_wd:
    COMP(R0, R5);
    IF LT JUMP _pa_wu;
    R0 = R0 - R5;
    JUMP _pa_wd;
_pa_wu:
    R0 = PASS R0;
    IF GE JUMP _pa_wok;
    R0 = R0 + R5;
    JUMP _pa_wu;
_pa_wok:
    R1 = R0 + 1;
    R2 = R5 - 1;
    R1 = MIN(R1, R2);
    R2 = DM(S_W, I3);
    R2 = LSHIFT R2 BY 1;                  // bytes per frame
    R0 = R0 * R2 (SSI);
    R0 = R0 + R13;
    DM(S_FA, I0) = R0;
    R1 = R1 * R2 (SSI);
    R1 = R1 + R13;
    DM(S_FB, I0) = R1;
    // the slot's side: +1 or -1, halved for slots 2 and 3
    R0 = FEXT R4 BY 0:1;
    R1 = DM(K_ONE, I1);
    R2 = DM(K_M1, I1);
    R0 = PASS R0;
    IF NE R1 = PASS R2;
    F9 = PASS F1;
    R0 = FEXT R4 BY 1:1;
    R0 = PASS R0;
    IF EQ JUMP _pa_p0;
    R1 = DM(K_HALF, I1);
    F9 = F9 * F1;
_pa_p0:
    R0 = DM(S_PIT, I0);
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
    R1 = DM(S_W, I3);
    F1 = FLOAT R1;
    F0 = F0 * F1;                         // table samples per output sample
    DM(S_INC, I0) = R0;
    R1 = PASS R8;
    R3 = LSHIFT R8 BY -1;
    R0 = FEXT R4 BY 1:1;
    R0 = PASS R0;
    IF NE R1 = PASS R3;                   // slots 2 and 3: half the spread
    R0 = FEXT R4 BY 0:1;
    R0 = PASS R0;
    IF NE R1 = -R1;
    R2 = 128;
    R1 = R1 + R2;                         // the pan step: 0 hard left .. 256 hard right
    R3 = _pa_pan;
    R0 = LSHIFT R1 BY 2;
    R0 = R0 + R3;
    I4 = R0;
    R0 = DM(0, I4);
    R2 = DM(K_GAIN, I1);
    F0 = F0 * F2;
    DM(S_GL, I0) = R0;
    R2 = PAN_N;
    R1 = R2 - R1;
    R0 = LSHIFT R1 BY 2;
    R0 = R0 + R3;
    I4 = R0;
    R0 = DM(0, I4);
    R2 = DM(K_GAIN, I1);
    F0 = F0 * F2;
    DM(S_GR, I0) = R0;
_pa_vnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _pa_vk;

    // ATK's step and DEC's factor (the render loop below takes I1, I5 and R3-R6)
    R0 = DM(2, I5);
    R0 = LSHIFT R0 BY -8;
    R0 = LSHIFT R0 BY 2;
    R1 = _pa_atk;
    R0 = R0 + R1;
    I4 = R0;
    R0 = DM(0, I4);
    DM(S_AI, I3) = R0;
    R0 = DM(3, I5);
    R0 = LSHIFT R0 BY -8;
    R0 = LSHIFT R0 BY 2;
    R1 = _pa_dec;
    R0 = R0 + R1;
    I4 = R0;
    R0 = DM(0, I4);
    DM(S_DC, I3) = R0;
    R0 = DM(K_OFF, I1);
    DM(S_OFF, I3) = R0;

    // render each sounding slot into both channels
    R14 = DM(S_W, I3);
    F13 = FLOAT R14;
    R15 = DM(S_ODD, I3);
    R11 = 0xffff0000;
    R12 = 0;
_pa_rk:
    R0 = LSHIFT R12 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I1 = R0;
    R0 = DM(S_STG, I1);
    R0 = PASS R0;
    IF EQ JUMP _pa_rnext;
    // the envelope's 32 values: F2 the level, F3 ATK's step, F4 DEC's factor, F5 1, F6 the stop level
    R2 = DM(S_LVL, I1);
    R3 = DM(S_AI, I3);
    R4 = DM(S_DC, I3);
    R5 = 0x3f800000;
    R6 = DM(S_OFF, I3);
    R1 = I3;
    R10 = S_ENV;
    R10 = LSHIFT R10 BY 2;
    R1 = R1 + R10;
    I4 = R1;
    I2 = R1;
    R10 = DK_BLOCK;
_pa_e:
    R1 = 1;
    COMP(R0, R1);
    IF NE JUMP _pa_ed;
    F2 = F2 + F3;
    COMP(F2, F5);
    IF LT JUMP _pa_es;
    F2 = PASS F5;
    R0 = 2;
    JUMP _pa_es;
_pa_ed:
    R1 = 3;
    COMP(R0, R1);
    IF NE JUMP _pa_es;
    F2 = F2 * F4;
    COMP(F2, F6);
    IF GE JUMP _pa_es;
    R2 = 0;
    R0 = 0;
_pa_es:
    DM(I4, M6) = R2;
    R10 = R10 - 1;
    IF NE JUMP _pa_e;
    DM(S_STG, I1) = R0;
    DM(S_LVL, I1) = R2;
    // the samples
    R6 = DM(S_FA, I1);
    R7 = DM(S_FB, I1);
    R9 = DM(S_PH, I1);
    COMP(F9, F13);
    IF GE R9 = R9 - R9;                   // a shorter table since the last block
    R0 = DM(S_OUT, I3);
    I0 = R0;
    R1 = 0x80;
    R0 = R0 + R1;
    I5 = R0;
    LCNTR = DK_BLOCK, DO _pa_s UNTIL LCE;
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
    R8 = DM(I2, M6);
    F5 = F5 * F8;                         // x the envelope
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
_pa_s: IF GE F9 = F9 - F13;
    DM(S_PH, I1) = R9;
_pa_rnext:
    R12 = R12 + 1;
    R0 = 4;
    COMP(R12, R0);
    IF LT JUMP _pa_rk;

    R0 = DK_DONE;
    R1 = DM(S_STG, I3);
    R2 = DM(S_STG + 1, I3);
    R1 = R1 OR R2;
    R2 = DM(S_STG + 2, I3);
    R1 = R1 OR R2;
    R2 = DM(S_STG + 3, I3);
    R1 = R1 OR R2;
    R1 = PASS R1;
    IF NE R0 = R0 - R0;                   // DK_OK (0) while a slot sounds
    JUMP (M13, I13);

_pa_out:
    R0 = DK_OK;
    JUMP (M13, I13);

// the next slot takes the note: pitch, phase 0, the attack from its level
_pa_note_on:
    // note 1, then core/polynotes' notes 2-4 (frame word 61 + 0x18 t: a byte each from the low one, 0x40 + the interval from
    // note 1; 0, 0x40 and 0x80 up are none): each takes the next slot (0, 1, 2, 3, 0, ...)
    I3 = R4;
    I2 = R8;
    R8 = DM(DK_IN_W_PITCH, I2);
    R2 = DM(_dv_g + 12);
    R4 = 0x60;
    R2 = R2 * R4 (SSI);
    R4 = _fw_frame;
    R2 = R2 + R4;
    I4 = R2;
    R9 = DM(61, I4);
    R9 = LSHIFT R9 BY 8;
    R0 = 0x40;
    R9 = R9 OR R0;                        // note 1 as the low byte
    R10 = 4;
_pa_nn:
    R0 = FEXT R9 BY 0:8;
    R9 = LSHIFT R9 BY -8;
    R0 = PASS R0;
    IF EQ JUMP _pa_nx;
    R1 = 0x80;
    COMP(R0, R1);
    IF GE JUMP _pa_nx;
    R1 = 0x40;
    R0 = R0 - R1;                         // the interval
    IF NE JUMP _pa_nt;
    R1 = 4;
    COMP(R10, R1);
    IF NE JUMP _pa_nx;
_pa_nt:
    R0 = LSHIFT R0 BY 8;
    R0 = R0 + R8;
    R4 = DM(S_NEXT, I3);
    R1 = R4 + 1;
    R2 = 3;
    R1 = R1 AND R2;
    DM(S_NEXT, I3) = R1;
    R4 = LSHIFT R4 BY 2;
    R1 = I3;
    R4 = R4 + R1;
    I4 = R4;
    DM(S_PIT, I4) = R0;
    R0 = 1;
    DM(S_STG, I4) = R0;
    R0 = 0;
    DM(S_PH, I4) = R0;
_pa_nx:
    R10 = R10 - 1;
    IF NE JUMP _pa_nn;
    R0 = DK_OK;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
// magic, contract, n_params, voices, state_bytes, state_clear_bytes, max_cycles_block, max_cycles_note_on,
// max_call_depth, stack_bytes
.GLOBAL _pa_desc;
.VAR _pa_desc = DK_MAGIC;
.VAR _pa_desc1 = DK_MACHINE_V1;
.VAR _pa_desc2 = 6;
.VAR _pa_desc3 = DK_MAX_VOICES;
.VAR _pa_desc4 = 512;
.VAR _pa_desc5 = 0;
.VAR _pa_desc6 = 13000;
.VAR _pa_desc7 = 200;
.VAR _pa_desc8 = 2;
.VAR _pa_desc9 = 0;
