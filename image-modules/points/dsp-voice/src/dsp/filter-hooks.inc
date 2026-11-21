// Stock converter and dispatch sites. The link writes the coefficient PC last;
// either stage skips a module type while its coefficient PC is zero.
#include "dk_filter.inc"
.SECTION/PM seg_pmco;
.GLOBAL _flt_convert;
.GLOBAL _flt_coef_hook;
.GLOBAL _flt_proc_hook;
.EXTERN _flt_convert_next;
.EXTERN _flt_coef_next;
.EXTERN _flt_coef_skip;
.EXTERN _flt_proc_next;
.EXTERN _flt_proc_skip;
.EXTERN _dl_filter_rows;

_flt_convert:
    DM(_flt_tmp) = R0;
    DM(_flt_tmp + 4) = R1;
    R0 = M0;
    R1 = 16;
    COMP(R0,R1);
    IF LT JUMP _flt_convert_stock;
    R1 = 32;
    COMP(R0,R1);
    IF GE JUMP _flt_convert_stock;
#ifdef FILTER_DN2
    S0 = R0;
#else
    S3 = R0;
#endif
    JUMP _flt_float;
_flt_convert_stock:
#ifdef FILTER_DN2
    S0 = DM(M0,I5);
#else
    S3 = DM(M0,I4);
#endif
_flt_float:
    R0 = DM(_flt_tmp);
    R1 = DM(_flt_tmp + 4);
#ifdef FILTER_DN2
    F8 = FLOAT R8;
#else
    F10 = FLOAT R10 BY R11;
#endif
    JUMP _flt_convert_next;

_flt_coef_hook:
    R12 = M4;
    DM(_flt_tmp) = R0;
    R0 = 7;
    COMP(R12,R0);
    R0 = DM(_flt_tmp);
    IF LT JUMP _flt_coef_stock;
    DM(_flt_tmp) = R0;
    R0 = 16;
    COMP(R12,R0);
    R0 = DM(_flt_tmp);
    IF LT JUMP _flt_coef_skip;
    JUMP _flt_coef_module;
_flt_coef_stock:
    R12 = R12 - R12, I12 = DM(M4,I4);
    JUMP _flt_coef_next;

_flt_proc_hook:
    IF LT JUMP _flt_proc_next;             // the displaced unsigned range check
    DM(_flt_tmp) = R0;
    R0 = 16;
    COMP(R2,R0);
    IF LT JUMP _flt_proc_absent;
    R0 = 32;
    COMP(R2,R0);
    IF GE JUMP _flt_proc_absent;
    R0 = DM(_flt_tmp);
    JUMP _flt_proc_module;
_flt_proc_absent:
    R0 = DM(_flt_tmp);
    JUMP _flt_proc_skip;

// Keep the stock loop context around a call. The saved type is M4 in both loops.
// In the process loop the range check has already consumed its flags.
_flt_coef_module:
    DM(_flt_save) = R0;
    R0 = DM(_dl_slots + DK_SLOT_FILTER_COEF);
    R0 = PASS R0;
    IF EQ JUMP _flt_coef_absent;
    DM(_flt_entry) = R0;
    R0 = 0;
    DM(_flt_stage) = R0;
    JUMP _flt_enter;
_flt_coef_absent:
    R0 = DM(_flt_save);
    JUMP _flt_coef_skip;

_flt_proc_module:
    DM(_flt_save) = R0;
    R0 = DM(_dl_slots + DK_SLOT_FILTER_PROC);
    R0 = PASS R0;
    IF EQ JUMP _flt_proc_absent;
    DM(_flt_entry) = R0;
    R0 = 1;
    DM(_flt_stage) = R0;
_flt_enter:
    DM(_flt_save + 4) = R1;
    DM(_flt_save + 8) = R2;
    DM(_flt_save + 12) = R3;
    DM(_flt_save + 16) = R4;
    DM(_flt_save + 20) = R5;
    DM(_flt_save + 24) = R6;
    DM(_flt_save + 28) = R7;
    DM(_flt_save + 32) = R8;
    DM(_flt_save + 36) = R9;
    DM(_flt_save + 40) = R10;
    DM(_flt_save + 44) = R11;
    DM(_flt_save + 48) = R12;
    DM(_flt_save + 52) = R13;
    DM(_flt_save + 56) = R14;
    DM(_flt_save + 60) = R15;
    DM(_flt_save + 64) = I0;
    DM(_flt_save + 68) = I1;
    DM(_flt_save + 72) = I2;
    DM(_flt_save + 76) = B3;
    DM(_flt_save + 80) = I3;
    DM(_flt_save + 84) = I4;
    DM(_flt_save + 88) = I5;
    DM(_flt_save + 92) = I12;
    DM(_flt_save + 96) = I13;
    DM(_flt_save + 100) = M1;
    DM(_flt_save + 104) = M2;
    DM(_flt_save + 108) = M3;
    DM(_flt_save + 112) = M4;
    DM(_flt_save + 116) = M13;

    R0 = MODE1;
    DM(_flt_mode) = R0;
    BIT CLR MODE1 0x1000;                  // restore context constants before enabling interrupts
    NOP;
    R0 = M4;
    R1 = 16;
    R0 = R0 - R1;
    R0 = LSHIFT R0 BY 5;                   // 32 bytes per filter type
    R1 = _dl_filter_rows;
    R0 = R0 + R1;
    I4 = R0;
    R1 = DM(0,I4);
    R1 = PASS R1;
    IF EQ JUMP _flt_leave;
    R1 = DM(_flt_stage);
    R1 = PASS R1;
    IF NE JUMP _flt_process_args;
    R12 = I0;
#ifdef FILTER_DN2
    R4 = R3;
    R1 = 0x418;                            // I0 at the lookup minus the current parameter-record base
#else
    R4 = R13;
    R1 = 0x160;
#endif
    R12 = R12 - R1;
    JUMP _flt_call;
_flt_process_args:
#ifdef FILTER_DN2
    R4 = 16;
    R4 = R4 - R15;
    R8 = I5;
#else
    R4 = 32;
    R4 = R4 - R7;
    R8 = I3;
#endif
_flt_call:
    M13 = 0;
    I12 = DM(_flt_entry);
    I13 = _flt_leave;
    JUMP (M13,I12);

_flt_leave:
    R1 = DM(_flt_save + 4);
    R2 = DM(_flt_save + 8);
    R3 = DM(_flt_save + 12);
    R4 = DM(_flt_save + 16);
    R5 = DM(_flt_save + 20);
    R6 = DM(_flt_save + 24);
    R7 = DM(_flt_save + 28);
    R8 = DM(_flt_save + 32);
    R9 = DM(_flt_save + 36);
    R10 = DM(_flt_save + 40);
    R11 = DM(_flt_save + 44);
    R12 = DM(_flt_save + 48);
    R13 = DM(_flt_save + 52);
    R14 = DM(_flt_save + 56);
    R15 = DM(_flt_save + 60);
    I0 = DM(_flt_save + 64);
    I1 = DM(_flt_save + 68);
    I2 = DM(_flt_save + 72);
    B3 = DM(_flt_save + 76);
    I3 = DM(_flt_save + 80);
    I4 = DM(_flt_save + 84);
    I5 = DM(_flt_save + 88);
    I12 = DM(_flt_save + 92);
    I13 = DM(_flt_save + 96);
    M1 = DM(_flt_save + 100);
    M2 = DM(_flt_save + 104);
    M3 = DM(_flt_save + 108);
    M4 = DM(_flt_save + 112);
    M13 = DM(_flt_save + 116);

    R0 = DM(_flt_mode);
    MODE1 = R0;
    R0 = DM(_flt_stage);
    R0 = PASS R0;
    R0 = DM(_flt_save);
    IF EQ JUMP _flt_coef_skip;
    JUMP _flt_proc_skip;

.SECTION/DM seg_dmda;
.PKDATA _flt_tmp, 8;
.PKDATA _flt_save, 120;
.PKDATA _flt_stage, 4;
.PKDATA _flt_entry, 4;
.PKDATA _flt_mode, 4;
