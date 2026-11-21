#include "dk_filter.inc"
#include "dsp/contract/call.inc"
#include "dsp/util/recip.inc"
#include "dsp/util/exp2.inc"
#include "dsp/util/tan.inc"
#define DK_BLOCK DK_FILTER_BLOCK
#include "dsp/filter/svf.inc"
#include "dsp/filter/rbank.inc"

// Per-voice state, in 32-bit words:
//   0..103   SDK resonator bank state
//   104..106 FREQ, SPRD and Q for the next block
//   107..108 input and output addresses for the SDK kernel
//   109..140 copy of the voice's input block
#define RBANK_PARAMS 104
#define RBANK_ARGS 107
#define RBANK_INPUT 109
#define RBANK_STATE_BYTES 564

.SECTION/PM seg_pmco;
.GLOBAL _rbnk_coef;
.GLOBAL _rbnk_process;

_rbnk_coef:
    R0 = LSHIFT R4 BY -2;             // L2 byte to normal-word alias
    R1 = RBANK_PARAMS;
    R0 = R0 + R1;
    I0 = R0;
    I1 = R12;                         // adapter's two knob words, at a byte address
    R2 = 0x46fe0000;                  // 32512.0: full-scale DK_P_PERCENT
    F8 = F8 * F2;                     // normalised, modulated stock cutoff
    R8 = FIX F8;
    R1 = DM(0, I1);
    R2 = DM(1, I1);
    DM(0, I0) = R8;
    DM(1, I0) = R1;
    DM(2, I0) = R2;
    R0 = DK_FILTER_OK;
    JUMP (M13, I13);

_rbnk_process:
    R4 = LSHIFT R4 BY -2;             // L2 per-voice state
    R3 = R8;                          // keep the stock buffer's byte alias, including DDR
    R0 = RBANK_INPUT;
    R0 = R4 + R0;
    R8 = RBANK_ARGS;
    R8 = R4 + R8;
    I2 = R8;
    I0 = R3;
    I1 = R0;
    DM(0, I2) = R0;
    DM(1, I2) = R3;
    R12 = RBANK_PARAMS;
    R12 = R4 + R12;
    // The adapter processes in place. Give the kernel an input it can reread
    // for every band while it writes its sum back to the stock buffer.
    LCNTR = DK_FILTER_BLOCK, DO _rbnk_copy UNTIL LCE;
        R0 = DM(I0, M6);
_rbnk_copy: DM(I1, M6) = R0;
    DK_CALL(_dk_rbank, _rbnk_after);
    R0 = DK_FILTER_OK;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
.GLOBAL _rbnk_desc;
.VAR _rbnk_desc = DK_FILTER_MAGIC;
.VAR _rbnk_desc1 = DK_FILTER_V1;
.VAR _rbnk_desc2 = DK_FILTER_MAX_PARAMS;
.VAR _rbnk_desc3 = RBANK_STATE_BYTES;
.VAR _rbnk_desc4 = 1;                 // _dk_svf_band uses one hardware CALL frame
.VAR _rbnk_desc5 = 72;                // DK_CALL's two words plus the kernel's 16
