#include "dk_filter.inc"

.SECTION/PM seg_pmco;
.GLOBAL _fgan_coef;
.GLOBAL _fgan_process;

_fgan_coef:
    I0 = R12;
    R0 = DM(0, I0);
    F0 = FLOAT R0;
    R1 = 0x38010204;              // 1 / (127 * 256)
    F0 = F0 * F1;
    I0 = R4;
    DM(0, I0) = R0;
    R0 = DK_FILTER_OK;
    JUMP (M13, I13);

_fgan_process:
    I0 = R4;
    R1 = DM(0, I0);
    I0 = R8;
    I1 = R8;
    LCNTR = DK_FILTER_BLOCK, DO _fgan_loop UNTIL LCE;
        R0 = DM(I0, M6);
        F0 = F0 * F1;
_fgan_loop: DM(I1, M6) = R0;
    R0 = DK_FILTER_OK;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
.GLOBAL _fgan_desc;
.VAR _fgan_desc = DK_FILTER_MAGIC;
.VAR _fgan_desc1 = DK_FILTER_V1;
.VAR _fgan_desc2 = 1;
.VAR _fgan_desc3 = 4;
.VAR _fgan_desc4 = 0;
.VAR _fgan_desc5 = 0;
