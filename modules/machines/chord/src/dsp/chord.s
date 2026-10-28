// machines/chord: the slot's sample as a one-shot played by up to four voices at set intervals, as a stereo voice of the SDK
// contract (sdk/dk_machine.h; [[voice]] stereo: 32 left samples at out, then 32 right ones). tools/patchkit/chord_ref.py is the
// model, operation for operation.
//
// Knobs (params): 0 LOOP, 1 DTUN, 2-4 INT2-INT4, 5 SPRD (knob H; the row's `lev` gives the track a fixed level).
//   sample  word 0 of the even voice's record is the left channel's first sample (a byte address), of the odd voice's record
//           the right one's (the same for a mono sample), +0x188 the frames; 16-bit, read as the aligned word that holds them
//   LOOP    bipolar, 0x4000 OFF (one-shot: a voice ends at the sample's end); above, a forward loop from the loop start to the
//           end; below, ping-pong between the loop start and the last frame. One step from the centre loops from the start;
//           the loop start is (distance - 0x100) / 0x3E00 of the sample, at most two frames before the end
//   voices  voice 1 plays the note; voice k (2-4) plays it INTk semitones away (0x4000 OFF, 0x4100 the note itself); detune
//           and spread alternate over the sounding voices as ChordWT's; each plays at 2^((pitch - NOTE0) / 3072) frames per
//           sample from frame 0 at the trig
//   out     one voice at the centre plays as loud as a stock sample machine; n voices 1 / sqrt(n) each
// The routine returns DK_DONE when every voice has ended.
//
// State (256 bytes, loaded as zeros): words S_* below, per voice k at S_x + k.

#include "dk_machine.inc"
#include "tables.inc"

#define S_POS 0
#define S_BACK 4
#define S_ALIVE 8
#define S_RATE 12
#define S_GL 16
#define S_GR 20
#define S_ON 24
#define S_OUT 28
#define S_AL 29
#define S_AR 30
#define S_N 31
#define S_LS 32
#define S_MODE 33
#define S_NF 34
#define S_LSF 35
#define S_LAST 36
#define REC_FRAMES 98

.SECTION/PM seg_pmco;
.GLOBAL _ch_render;
.GLOBAL _ch_note_on;
.EXTERN _dv_exp2;
.EXTERN _dv_g;
.EXTERN _ch_k;
.EXTERN _ch_pan;

_ch_render:
    I3 = R4;
    I2 = R8;
    I5 = R12;
    R0 = _ch_k;
    I1 = R0;

    R0 = DM(DK_IN_W_OUT, I2);
    DM(S_OUT, I3) = R0;
    I0 = R0;
    R1 = 0;
    LCNTR = 64, DO _ch_z UNTIL LCE;
_ch_z: DM(I0, M6) = R1;

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
    IF EQ JUMP _ch_ok;
    R0 = 4;
    COMP(R11, R0);
    IF LT JUMP _ch_ok;
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
    IF LT JUMP _ch_lset;
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
_ch_lset:
    DM(S_MODE, I3) = R2;
    DM(S_LS, I3) = R3;
    F0 = FLOAT R3;
    DM(S_LSF, I3) = R0;

    // the sounding voices
    R0 = 0x4000;
    R4 = 1;
    R8 = DM(S_ALIVE, I3);
    DM(S_ON, I3) = R8;
    R5 = 1;                               // k
_ch_on:
    R2 = R5 + 1;
    R2 = LSHIFT R2 BY 2;
    R3 = I5;
    R2 = R2 + R3;
    I4 = R2;
    R2 = DM(0, I4);                       // INTk
    R3 = LSHIFT R5 BY 2;
    R1 = I3;
    R3 = R3 + R1;
    I4 = R3;
    R1 = DM(S_ALIVE, I4);
    COMP(R2, R0);
    IF EQ R1 = R1 - R1;
    DM(S_ON, I4) = R1;
    DM(S_ALIVE, I4) = R1;                 // a voice switched off ends (it comes back at the next trig)
    R8 = R8 + R1;
    R5 = R5 + 1;
    R1 = 4;
    COMP(R5, R1);
    IF LT JUMP _ch_on;
    R8 = PASS R8;
    IF EQ JUMP _ch_end;

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

    // per voice k (R4), j (R5) = the sounding voices before it: the rate and the two gains
    R4 = 0;
    R5 = 0;
_ch_vk:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I0 = R0;
    R0 = DM(S_ON, I0);
    R0 = PASS R0;
    IF EQ JUMP _ch_vnext;
    R0 = FEXT R5 BY 0:1;
    R1 = DM(K_ONE, I1);
    R2 = DM(K_M1, I1);
    R0 = PASS R0;
    IF NE R1 = PASS R2;
    F9 = PASS F1;                         // the voice's side: +1 or -1
    R0 = FEXT R5 BY 1:1;
    R0 = PASS R0;
    IF EQ JUMP _ch_p0;
    R1 = DM(K_HALF, I1);
    F9 = F9 * F1;                         // the second pair: half the detune
_ch_p0:
    R1 = 0;
    R2 = PASS R4;
    IF EQ JUMP _ch_k0;
    R2 = R4 + 1;
    R2 = LSHIFT R2 BY 2;
    R3 = I5;
    R2 = R2 + R3;
    I4 = R2;
    R1 = DM(0, I4);
    R2 = 0x4000;
    R1 = R1 - R2;
    R2 = 0x100;
    R3 = R1 - R2;
    R1 = PASS R1;
    IF GT R1 = PASS R3;                   // the interval in pitch words: UNI is 0
_ch_k0:
    R0 = DM(DK_IN_W_PITCH, I2);
    R0 = R0 + R1;
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
    R3 = _ch_pan;
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
_ch_vnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _ch_vk;

    // render each sounding voice into both channels
    R4 = 0;
_ch_rk:
    R0 = LSHIFT R4 BY 2;
    R1 = I3;
    R0 = R0 + R1;
    I1 = R0;                              // the voice's state
    R0 = DM(S_ON, I1);
    R0 = PASS R0;
    IF EQ JUMP _ch_rnext;
    R10 = DM(S_POS, I1);
    R0 = DM(S_OUT, I3);
    I0 = R0;
    R1 = 0x80;
    R0 = R0 + R1;
    I5 = R0;
    R15 = DK_BLOCK;
_ch_s:
    R0 = DM(S_MODE, I3);
    R0 = PASS R0;
    IF LT JUMP _ch_pp;
_ch_fw:
    R1 = DM(S_NF, I3);
    COMP(F10, F1);
    IF LT JUMP _ch_rd;
    R0 = PASS R0;
    IF EQ JUMP _ch_kill;
    R2 = DM(S_LSF, I3);
    F1 = F1 - F2;
    F10 = F10 - F1;
    JUMP _ch_fw;
_ch_pp:
    R1 = DM(S_LAST, I3);
    COMP(F10, F1);
    IF LE JUMP _ch_pplo;
    F2 = F1 + F1;
    F10 = F2 - F10;
    R2 = 1;
    DM(S_BACK, I1) = R2;
    JUMP _ch_pp;
_ch_pplo:
    R1 = DM(S_LSF, I3);
    COMP(F10, F1);
    IF GE JUMP _ch_rd;
    F2 = F1 + F1;
    F10 = F2 - F10;
    R2 = 0;
    DM(S_BACK, I1) = R2;
    JUMP _ch_pp;
_ch_rd:
    R6 = TRUNC F10;
    F0 = FLOAT R6;
    F7 = F10 - F0;                        // the fraction
    R5 = R6 + 1;
    R0 = DM(S_N, I3);
    COMP(R5, R0);
    IF LT JUMP _ch_k1;
    R5 = PASS R6;
    R0 = DM(S_MODE, I3);
    R0 = PASS R0;
    IF LE JUMP _ch_k1;
    R5 = DM(S_LS, I3);                    // a forward loop goes on at the loop start
_ch_k1:
    R0 = DM(S_AL, I3);
    R1 = PASS R6;
    CALL _ch_get;
    F8 = PASS F0;
    R0 = DM(S_AL, I3);
    R1 = PASS R5;
    CALL _ch_get;
    F0 = F0 - F8;
    F0 = F0 * F7;
    F8 = F8 + F0;
    R1 = DM(S_GL, I1);
    F1 = F8 * F1;
    R2 = DM(0, I0);
    F2 = F2 + F1;
    DM(I0, M6) = R2;
    R0 = DM(S_AL, I3);
    R1 = DM(S_AR, I3);
    COMP(R0, R1);
    IF EQ JUMP _ch_rr;
    R0 = PASS R1;
    R1 = PASS R6;
    CALL _ch_get;
    F8 = PASS F0;
    R0 = DM(S_AR, I3);
    R1 = PASS R5;
    CALL _ch_get;
    F0 = F0 - F8;
    F0 = F0 * F7;
    F8 = F8 + F0;
_ch_rr:
    R1 = DM(S_GR, I1);
    F1 = F8 * F1;
    R2 = DM(0, I5);
    F2 = F2 + F1;
    DM(I5, M6) = R2;
    R1 = DM(S_RATE, I1);
    R0 = DM(S_BACK, I1);
    R0 = PASS R0;
    IF NE JUMP _ch_bk;
    F10 = F10 + F1;
    JUMP _ch_nx;
_ch_bk:
    F10 = F10 - F1;
_ch_nx:
    R15 = R15 - 1;
    IF NE JUMP _ch_s;
    DM(S_POS, I1) = R10;
    JUMP _ch_rnext;
_ch_kill:
    R0 = 0;
    DM(S_ALIVE, I1) = R0;
    DM(S_POS, I1) = R10;
_ch_rnext:
    R4 = R4 + 1;
    R0 = 4;
    COMP(R4, R0);
    IF LT JUMP _ch_rk;

_ch_end:
    R0 = DK_DONE;
    R1 = DM(S_ALIVE, I3);
    R2 = DM(S_ALIVE + 1, I3);
    R1 = R1 OR R2;
    R2 = DM(S_ALIVE + 2, I3);
    R1 = R1 OR R2;
    R2 = DM(S_ALIVE + 3, I3);
    R1 = R1 OR R2;
    R1 = PASS R1;
    IF NE R0 = R0 - R0;                   // DK_OK (0) while a voice sounds
    JUMP (M13, I13);
_ch_ok:
    R0 = DK_OK;
    JUMP (M13, I13);

// F0 = the sample R1 of the channel at byte address R0, as int16 x 2^16 in a float; clobbers R1-R3, I4
_ch_get:
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

// a trig: every voice from frame 0, forwards
_ch_note_on:
    I3 = R4;
    R0 = 0;
    R1 = 1;
    LCNTR = 4, DO _ch_n UNTIL LCE;
    DM(S_POS, I3) = R0;
    DM(S_BACK, I3) = R0;
    DM(S_ALIVE, I3) = R1;
_ch_n: MODIFY(I3, 4);
    R0 = DK_OK;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
// magic, contract, n_params, voices, state_bytes, state_clear_bytes, max_cycles_block, max_cycles_note_on,
// max_call_depth, stack_bytes
.GLOBAL _ch_desc;
.VAR _ch_desc = DK_MAGIC;
.VAR _ch_desc1 = DK_MACHINE_V1;
.VAR _ch_desc2 = 6;
.VAR _ch_desc3 = DK_MAX_VOICES;
.VAR _ch_desc4 = 256;
.VAR _ch_desc5 = 0;
.VAR _ch_desc6 = 20000;
.VAR _ch_desc7 = 60;
.VAR _ch_desc8 = 2;
.VAR _ch_desc9 = 0;
