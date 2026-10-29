// machines/parasmp: the slot's sample as a one-shot played paraphonically: each trig takes the next of four voices, with its own
// note and amp envelope; a stereo voice of the SDK contract (sdk/dk_machine.h; [[voice]] stereo: 32 left samples at out, then
// 32 right ones). tools/patchkit/parasmp_ref.py is the model, operation for operation.
//
// Knobs (params): 0 LOOP, 1 DTUN, 2 ATK, 3 DEC, 4 STRT, 5 SPRD (knob H; the row's `lev` gives the track a fixed level).
//   sample  the even and odd voices' records give the left and right channels and the frames
//   LOOP    0x4000 OFF (a voice ends at the sample's end), above forward, below ping-pong
//   voices  note_on gives the next slot (0, 1, 2, 3, 0, ...) the note and the attack; the slot starts at STRT x the frames at
//           its first block. Envelope as ParaWT's: attack to 1, held until the track's note-off (the release latch, byte
//           v = 2t), then DEC's factor per sample, stopped below -80 dB
//   DTUN    slot k at +/- d^3 semitones (+, -, +1/2, -1/2); SPRD slot k at right, left, right/2, left/2 (equal power)
//   out     the voices' sum, each 6 dB below a stock sample machine playing the slot
// render returns DK_DONE when every slot has stopped.
//
// State (512 bytes, a power of two for the track index; loaded as zeros): words S_* below, per slot k at S_x + k.

#include "dk_machine.inc"
#include "tables.inc"

#define S_POS 0
#define S_BACK 4
#define S_LVL 8
#define S_STG 12
#define S_PIT 16
#define S_RATE 20
#define S_GL 24
#define S_GR 28
#define S_NEW 32
#define S_NEXT 36
#define S_OUT 37
#define S_AL 38
#define S_AR 39
#define S_N 40
#define S_LS 41
#define S_MODE 42
#define S_NF 43
#define S_LSF 44
#define S_LAST 45
#define S_AI 46
#define S_DC 47
#define S_OFFL 48
#define S_START 49
#define S_ENV 64
#define REC_FRAMES 98

.SECTION/PM seg_pmco;
.GLOBAL _ps_render;
.GLOBAL _ps_note_on;
.EXTERN _dv_exp2;
.EXTERN _dv_g;
.EXTERN _fw_frame;
.EXTERN _ps_k;
.EXTERN _ps_pan;
.EXTERN _ps_atk;
.EXTERN _ps_dec;
.EXTERN _dkstate_parasmp;
.EXTERN _fw_release_latch;

_ps_render:
    I3 = R4;
    I2 = R8;
    I5 = R12;
    R0 = _ps_k;
    I1 = R0;

    R0 = DM(DK_IN_W_OUT, I2);
    DM(S_OUT, I3) = R0;
    I0 = R0;
    R1 = 0;
    LCNTR = 64, DO _ps_z UNTIL LCE;
_ps_z: DM(I0, M6) = R1;

    // the track's note-off (the release latch byte v = 2t): attacking and held slots decay
    R0 = _dkstate_parasmp;
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
    IF EQ JUMP _ps_held;
    R4 = 0;
_ps_rel:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I4 = R0;
    R0 = DM(S_STG, I4);
    R1 = 1;
    COMP(R0, R1);
    IF EQ JUMP _ps_rset;
    R1 = 2;
    COMP(R0, R1);
    IF NE JUMP _ps_relnext;
_ps_rset:
    R1 = 3;
    DM(S_STG, I4) = R1;                   // ATTACK or HELD: DECAY
_ps_relnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _ps_rel;
_ps_held:

    // the sample
    R0 = DM(_dv_g + 16);                  // the even voice's record
    I4 = R0;
    R10 = DM(0, I4);
    R11 = DM(REC_FRAMES, I4);
    R1 = 0x1d8;
    R0 = R0 + R1;
    I4 = R0;
    R12 = DM(0, I4);                      // the odd voice's record: the right channel
    R12 = PASS R12;
    IF EQ R12 = PASS R10;
    R10 = PASS R10;
    IF EQ JUMP _ps_ok;
    R0 = 4;
    COMP(R11, R0);
    IF LT JUMP _ps_ok;
    DM(S_AL, I3) = R10;
    DM(S_AR, I3) = R12;
    DM(S_N, I3) = R11;
    F0 = FLOAT R11;
    DM(S_NF, I3) = R0;
    R0 = R11 - 1;
    F0 = FLOAT R0;
    DM(S_LAST, I3) = R0;

    // LOOP: the mode (0 none, 1 forward, -1 ping-pong) and the loop start
    R0 = DM(0, I5);
    R1 = 0x4000;
    R0 = R0 - R1;
    R2 = 0;
    R3 = -1;
    R1 = ABS R0;
    R4 = 0x100;
    COMP(R1, R4);
    IF LT JUMP _ps_lset;
    R2 = 1;
    R0 = PASS R0;
    IF LT R2 = PASS R3;
    R1 = R1 - R4;
    F0 = FLOAT R1;
    R1 = DM(K_LP, I1);
    F0 = F0 * F1;
    R1 = DM(K_ONE, I1);
    F0 = MIN(F0, F1);
    F1 = FLOAT R11;
    F0 = F0 * F1;
    R3 = TRUNC F0;
    R1 = R11 - 1;
    R1 = R1 - 1;
    R3 = MIN(R3, R1);
_ps_lset:
    DM(S_MODE, I3) = R2;
    DM(S_LS, I3) = R3;
    F0 = FLOAT R3;
    DM(S_LSF, I3) = R0;

    // STRT: the frame a new slot starts at
    R0 = DM(4, I5);
    F0 = FLOAT R0;
    R1 = DM(K_W01, I1);
    F0 = F0 * F1;
    R1 = DM(K_ONE, I1);
    F0 = MIN(F0, F1);
    F1 = FLOAT R11;
    F0 = F0 * F1;
    R0 = TRUNC F0;
    R1 = R11 - 1;
    R0 = MIN(R0, R1);
    F0 = FLOAT R0;
    DM(S_START, I3) = R0;

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

    // per sounding slot k (R4): a new slot's start, its rate and its two gains
    R4 = 0;
_ps_vk:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I0 = R0;
    R0 = DM(S_STG, I0);
    R0 = PASS R0;
    IF EQ JUMP _ps_vnext;
    R0 = DM(S_NEW, I0);
    R0 = PASS R0;
    IF EQ JUMP _ps_old;
    R0 = DM(S_START, I3);
    DM(S_POS, I0) = R0;
    R0 = 0;
    DM(S_BACK, I0) = R0;
    DM(S_NEW, I0) = R0;
_ps_old:
    R0 = FEXT R4 BY 0:1;
    R1 = DM(K_ONE, I1);
    R2 = DM(K_M1, I1);
    R0 = PASS R0;
    IF NE R1 = PASS R2;
    F9 = PASS F1;
    R0 = FEXT R4 BY 1:1;
    R0 = PASS R0;
    IF EQ JUMP _ps_p0;
    R1 = DM(K_HALF, I1);
    F9 = F9 * F1;
_ps_p0:
    R0 = DM(S_PIT, I0);
    R1 = NOTE0;
    R0 = R0 - R1;
    F0 = FLOAT R0;
    F1 = F11 * F9;
    F0 = F0 + F1;
    R1 = DM(K_SEMI, I1);
    F0 = F0 * F1;
    R1 = DM(K_OCTMAX, I1);
    F0 = MIN(F0, F1);
    F1 = -F1;
    F0 = MAX(F0, F1);
    CALL _dv_exp2;
    DM(S_RATE, I0) = R0;
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
    R3 = _ps_pan;
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
_ps_vnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _ps_vk;

    // ATK's step and DEC's factor (the loop below takes I1 and I5)
    R0 = DM(2, I5);
    R0 = LSHIFT R0 BY -8;
    R0 = LSHIFT R0 BY 2;
    R1 = _ps_atk;
    R0 = R0 + R1;
    I4 = R0;
    R0 = DM(0, I4);
    DM(S_AI, I3) = R0;
    R0 = DM(3, I5);
    R0 = LSHIFT R0 BY -8;
    R0 = LSHIFT R0 BY 2;
    R1 = _ps_dec;
    R0 = R0 + R1;
    I4 = R0;
    R0 = DM(0, I4);
    DM(S_DC, I3) = R0;
    R0 = DM(K_OFF, I1);
    DM(S_OFFL, I3) = R0;

    // render each sounding slot into both channels
    R4 = 0;
_ps_rk:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I1 = R0;                              // the slot's state
    R0 = DM(S_STG, I1);
    R0 = PASS R0;
    IF EQ JUMP _ps_rnext;
    // the envelope's 32 values: F2 the level, F3 ATK's step, F12 DEC's factor, F5 1, F6 the stop level
    R2 = DM(S_LVL, I1);
    R3 = DM(S_AI, I3);
    R12 = DM(S_DC, I3);
    R5 = 0x3f800000;
    R6 = DM(S_OFFL, I3);
    R1 = I3;
    R10 = S_ENV;
    R10 = LSHIFT R10 BY 2;
    R1 = R1 + R10;
    I4 = R1;
    I2 = R1;
    R10 = DK_BLOCK;
_ps_e:
    R1 = 1;
    COMP(R0, R1);
    IF NE JUMP _ps_ed;
    F2 = F2 + F3;
    COMP(F2, F5);
    IF LT JUMP _ps_es;
    F2 = PASS F5;
    R0 = 2;
    JUMP _ps_es;
_ps_ed:
    R1 = 3;
    COMP(R0, R1);
    IF NE JUMP _ps_es;
    F2 = F2 * F12;
    COMP(F2, F6);
    IF GE JUMP _ps_es;
    R2 = 0;
    R0 = 0;
_ps_es:
    DM(I4, M6) = R2;
    R10 = R10 - 1;
    IF NE JUMP _ps_e;
    DM(S_STG, I1) = R0;
    DM(S_LVL, I1) = R2;
    // the samples
    R10 = DM(S_POS, I1);
    R0 = DM(S_OUT, I3);
    I0 = R0;
    R1 = 0x80;
    R0 = R0 + R1;
    I5 = R0;
    R15 = DK_BLOCK;
_ps_s:
    R0 = DM(S_MODE, I3);
    R0 = PASS R0;
    IF LT JUMP _ps_pp;
_ps_fw:
    R1 = DM(S_NF, I3);
    COMP(F10, F1);
    IF LT JUMP _ps_rd;
    R0 = PASS R0;
    IF EQ JUMP _ps_kill;
    R2 = DM(S_LSF, I3);
    F1 = F1 - F2;
    F10 = F10 - F1;
    JUMP _ps_fw;
_ps_pp:
    R1 = DM(S_LAST, I3);
    COMP(F10, F1);
    IF LE JUMP _ps_pplo;
    F2 = F1 + F1;
    F10 = F2 - F10;
    R2 = 1;
    DM(S_BACK, I1) = R2;
    JUMP _ps_pp;
_ps_pplo:
    R1 = DM(S_LSF, I3);
    COMP(F10, F1);
    IF GE JUMP _ps_rd;
    F2 = F1 + F1;
    F10 = F2 - F10;
    R2 = 0;
    DM(S_BACK, I1) = R2;
    JUMP _ps_pp;
_ps_rd:
    R6 = TRUNC F10;
    F0 = FLOAT R6;
    F7 = F10 - F0;                        // the fraction
    R5 = R6 + 1;
    R0 = DM(S_N, I3);
    COMP(R5, R0);
    IF LT JUMP _ps_k1;
    R5 = PASS R6;
    R0 = DM(S_MODE, I3);
    R0 = PASS R0;
    IF LE JUMP _ps_k1;
    R5 = DM(S_LS, I3);                    // a forward loop goes on at the loop start
_ps_k1:
    R9 = DM(I2, M6);                      // the envelope
    R0 = DM(S_AL, I3);
    R1 = PASS R6;
    CALL _ps_get;
    F8 = PASS F0;
    R0 = DM(S_AL, I3);
    R1 = PASS R5;
    CALL _ps_get;
    F0 = F0 - F8;
    F0 = F0 * F7;
    F8 = F8 + F0;
    F8 = F8 * F9;
    R1 = DM(S_GL, I1);
    F1 = F8 * F1;
    R2 = DM(0, I0);
    F2 = F2 + F1;
    DM(I0, M6) = R2;
    R0 = DM(S_AL, I3);
    R1 = DM(S_AR, I3);
    COMP(R0, R1);
    IF EQ JUMP _ps_rr;
    R0 = PASS R1;
    R1 = PASS R6;
    CALL _ps_get;
    F8 = PASS F0;
    R0 = DM(S_AR, I3);
    R1 = PASS R5;
    CALL _ps_get;
    F0 = F0 - F8;
    F0 = F0 * F7;
    F8 = F8 + F0;
    F8 = F8 * F9;
_ps_rr:
    R1 = DM(S_GR, I1);
    F1 = F8 * F1;
    R2 = DM(0, I5);
    F2 = F2 + F1;
    DM(I5, M6) = R2;
    R1 = DM(S_RATE, I1);
    R0 = DM(S_BACK, I1);
    R0 = PASS R0;
    IF NE JUMP _ps_bk;
    F10 = F10 + F1;
    JUMP _ps_nx;
_ps_bk:
    F10 = F10 - F1;
_ps_nx:
    R15 = R15 - 1;
    IF NE JUMP _ps_s;
    DM(S_POS, I1) = R10;
    JUMP _ps_rnext;
_ps_kill:
    R0 = 0;
    DM(S_STG, I1) = R0;
    DM(S_POS, I1) = R10;
_ps_rnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _ps_rk;

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
_ps_ok:
    R0 = DK_OK;
    JUMP (M13, I13);

// F0 = the sample R1 of the channel at byte address R0, as int16 x 2^16 in a float; clobbers R1-R3, I4
_ps_get:
    R2 = FEXT R0 BY 1:1;                  // 1 when the channel starts at an upper half
    R1 = R1 + R2;
    R3 = FEXT R1 BY 0:1;
    R1 = LSHIFT R1 BY -1;
    R1 = LSHIFT R1 BY 2;
    R2 = -4;
    R2 = R0 AND R2;
    R1 = R1 + R2;
    I4 = R1;
    R1 = DM(0, I4);
    R3 = BTGL R3 BY 0;
    R3 = LSHIFT R3 BY 4;                  // 16 for the lower half, 0 for the upper
    R1 = LSHIFT R1 BY R3;
    R2 = 0xffff0000;
    R1 = R1 AND R2;
    F0 = FLOAT R1;
    RTS;

// the next slot takes the note: pitch, the attack from its level, its start at its first block
_ps_note_on:
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
_ps_qn:
    R0 = FEXT R9 BY 0:8;
    R9 = LSHIFT R9 BY -8;
    R0 = PASS R0;
    IF EQ JUMP _ps_qx;
    R1 = 0x80;
    COMP(R0, R1);
    IF GE JUMP _ps_qx;
    R1 = 0x40;
    R0 = R0 - R1;                         // the interval
    IF NE JUMP _ps_qt;
    R1 = 4;
    COMP(R10, R1);
    IF NE JUMP _ps_qx;
_ps_qt:
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
    DM(S_NEW, I4) = R0;
_ps_qx:
    R10 = R10 - 1;
    IF NE JUMP _ps_qn;
    R0 = DK_OK;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
// magic, contract, n_params, voices, state_bytes, state_clear_bytes, max_cycles_block, max_cycles_note_on,
// max_call_depth, stack_bytes
.GLOBAL _ps_desc;
.VAR _ps_desc = DK_MAGIC;
.VAR _ps_desc1 = DK_MACHINE_V1;
.VAR _ps_desc2 = 6;
.VAR _ps_desc3 = DK_MAX_VOICES;
.VAR _ps_desc4 = 512;
.VAR _ps_desc5 = 0;
.VAR _ps_desc6 = 22000;
.VAR _ps_desc7 = 200;
.VAR _ps_desc8 = 2;
.VAR _ps_desc9 = 0;
