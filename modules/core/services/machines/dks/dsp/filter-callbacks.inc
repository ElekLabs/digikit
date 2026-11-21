// The image stub calls the service for a published filter row. R0 = row's
// byte address, R4 = voice ordinal, R8 = cutoff (coef) or block (process),
// R12 = parameter record's byte address (coef). I13 = stub continuation,
// M13 = 0. The stub restores the stock registers that a callback can clobber.
#include "dk_filter.inc"

#ifdef FILTER_DN2
#define FLT_KNOB2_WORD 116
#define FLT_KNOB3_WORD 114
#else
#define FLT_KNOB2_WORD 28
#define FLT_KNOB3_WORD 26
#endif

.SECTION/PM seg_pmco;
.GLOBAL _flt_coef;
.GLOBAL _flt_process;

_flt_coef:
    R5 = 0;
    JUMP _flt_enter;
_flt_process:
    R5 = 1;
_flt_enter:
    R3 = MODE1;
    DM(_flt_mode) = R3;
    BIT CLR MODE1 0x20A000;               // PEYEN, TRUNCATE, ALUSAT
    BIT SET MODE1 0x10000;                // RND32
    NOP;

    R3 = L4;
    DM(_flt_l4) = R3;
    L4 = 0;
    I4 = _flt_context;
    DM(0, I4) = I13;
    R3 = M5;
    DM(1, I4) = R3;
    R3 = M6;
    DM(2, I4) = R3;
    R3 = M7;
    DM(3, I4) = R3;
    DM(4, I4) = M14;
    DM(5, I4) = M15;
    R3 = L0;
    DM(6, I4) = R3;
    R3 = L1;
    DM(7, I4) = R3;
    R3 = L2;
    DM(8, I4) = R3;
    R3 = L3;
    DM(9, I4) = R3;
    R3 = L5;
    DM(10, I4) = R3;
    M5 = 0;
    M6 = 1;
    M7 = -1;
    M14 = 1;
    M15 = -1;
    L0 = 0;
    L1 = 0;
    L2 = 0;
    L3 = 0;
    L5 = 0;

    I0 = R0;
    DM(12, I4) = R0;                     // row address survives either callback
    R1 = LSHIFT R4 BY 2;
    R2 = _flt_bad_coef;
    R1 = R1 + R2;
    I1 = R1;
    R6 = 1;
    R6 = LSHIFT R6 BY R4;
    R7 = DM(5, I0);
    R2 = DM(2, I0);
    R3 = DM(3, I0);
    R4 = R4 * R3 (SSI);
    R4 = R4 + R2;                         // state base + voice * byte stride
    R9 = R7 AND R6;
    IF NE JUMP _flt_initialized;
    R2 = 0;
    DM(0, I1) = R2;                      // a reused row can still have a failed coefficient latched
    R9 = LSHIFT R3 BY -2;
    R9 = PASS R9;
    IF EQ JUMP _flt_mark_initialized;
    I2 = R4;
    LCNTR = R9, DO _flt_init_zero UNTIL LCE;
_flt_init_zero:
        DM(I2, M6) = R2;
_flt_mark_initialized:
    R7 = R7 OR R6;
    DM(5, I0) = R7;
_flt_initialized:
    R5 = PASS R5;
    IF NE JUMP _flt_run_process;

    DM(11, I4) = R1;                     // latch address survives coef
    I12 = DM(0, I0);
    R1 = DM(4, I0);                      // declared parameter count
    I0 = R12;
    R12 = _flt_params;
    I2 = R12;
    R6 = DM(FLT_KNOB2_WORD, I0);
    R7 = DM(FLT_KNOB3_WORD, I0);
    R2 = 0;
    R3 = 0x3f800000;                     // float 1.0, to clamp before FIX
    R6 = MIN(R6, R3);
    R7 = MIN(R7, R3);
    R6 = MAX(R6, R2);
    R7 = MAX(R7, R2);
    R2 = 0x47000000;                     // 32768.0: the record uses panel / 128
    R3 = 0x46fe0000;                     // 32512.0: the record uses panel / 127
    F6 = F6 * F2;
    F7 = F7 * F3;
    R6 = FIX F6;
    R7 = FIX F7;
    R3 = 32512;                          // maximum page word, 127 * 256
    R6 = MIN(R6, R3);
    R2 = 0;
    R3 = 1;
    COMP(R1, R3);
    IF LT R6 = PASS R2;
    R3 = DK_FILTER_MAX_PARAMS;
    COMP(R1, R3);
    IF LT R7 = PASS R2;
    DM(0, I2) = R6;
    DM(1, I2) = R7;
    I13 = _flt_coef_back;
    JUMP (M13, I12);

_flt_coef_back:
    I4 = _flt_context;
    I1 = DM(11, I4);
    R2 = DM(12, I4);
    R1 = DK_FILTER_OK;
    COMP(R0, R1);
    IF NE R1 = PASS R2;
    DM(0, I1) = R1;                      // failed coef: silence this row's next block for the voice
    JUMP _flt_leave;

_flt_run_process:
    DM(13, I4) = R8;                     // audio buffer survives the callback
    I12 = DM(1, I0);
    R1 = DM(0, I1);
    R2 = 0;
    DM(0, I1) = R2;
    COMP(R0, R1);
    IF EQ JUMP _flt_silence;
    R12 = DK_FILTER_BLOCK;
    I13 = _flt_process_back;
    JUMP (M13, I12);

_flt_process_back:
    I4 = _flt_context;
    R8 = DM(13, I4);
    R1 = DK_FILTER_OK;
    COMP(R0, R1);
    IF NE JUMP _flt_silence;
    I0 = R8;
    R2 = 0x7fffffff;
    R3 = 0;
    LCNTR = DK_FILTER_BLOCK, DO _flt_bound UNTIL LCE;
        R0 = DM(I0, M6);
        R0 = R0 AND R2;
_flt_bound:
        R3 = MAX(R3, R0);
    R0 = DK_FILTER_BOUND_BITS;
    COMP(R3, R0);                       // also catches all Inf and NaN bit patterns
    IF LE JUMP _flt_leave;
_flt_silence:
    I0 = R8;
    R0 = 0;
    LCNTR = DK_FILTER_BLOCK, DO _flt_zero UNTIL LCE;
_flt_zero:
        DM(I0, M6) = R0;

    I0 = DM(12, I4);
    R5 = DM(_dv_diag);                    // block heartbeat
    R6 = DM(6, I0);
    R7 = DM(7, I0);
    R7 = PASS R7;
    IF EQ JUMP _flt_new_window;
    R6 = R5 - R6;
    IF LT JUMP _flt_new_window;
    R3 = 1500;
    COMP(R6, R3);
    IF LT JUMP _flt_count_fault;
_flt_new_window:
    DM(6, I0) = R5;
    R7 = 0;
_flt_count_fault:
    R7 = R7 + 1;
    DM(7, I0) = R7;
    R3 = 8;
    COMP(R7, R3);
    IF LT JUMP _flt_leave;
    DM(0, I0) = R0;                      // zero coefficient PC disables both stages

_flt_leave:
    I13 = DM(0, I4);
    M5 = DM(1, I4);
    M6 = DM(2, I4);
    M7 = DM(3, I4);
    M14 = DM(4, I4);
    M15 = DM(5, I4);
    L0 = DM(6, I4);
    L1 = DM(7, I4);
    L2 = DM(8, I4);
    L3 = DM(9, I4);
    L5 = DM(10, I4);
    R0 = DM(_flt_l4);
    L4 = R0;
    R0 = DM(_flt_mode);
    MODE1 = R0;
    NOP;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
.VAR _flt_mode = 0;
.VAR _flt_l4 = 0;
.VAR _flt_context[14] = 0;
.VAR _flt_params[DK_FILTER_MAX_PARAMS] = 0;
.VAR _flt_bad_coef[32] = 0;               // failed coefficient's row address, one word per voice
