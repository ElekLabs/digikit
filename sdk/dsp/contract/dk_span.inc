// dk_span.inc: the routines behind DK_SPAN_BEGIN and DK_SPAN_END (dk_trace.inc; docs/plans/profiling.md 13.1). Hand-written. Include it once into the object of a
// service's dispatch code, in the program memory section, after defining
//
//   DK_SPAN_PTR_WORD   the DM word that holds the stats window's byte address, 0 while stats are off (the macros' PTR)
//   DK_SPAN_ROW_CODE   instructions that leave the stats row (0 .. DK_STATS_ROWS - 1) in R2 for the span just ended; they may use R0 and R2, and read
//                      what the dispatch code keeps in DM or in registers the span does not touch
//
// In: R3 = I1 = the window's byte address (the macros). _dk_span_begin keeps R0. _dk_span_end keeps R0 to R2 (it clobbers R3 and I1) and adds the span to its
// row: calls + 1, the cycles to the 64-bit sum, the largest call, the sum of the running block (core/dsp-link folds it into the largest block at the end of
// the block).

#include "dk_trace.inc"

_dk_span_begin:
    DM(DK_STATS_PRIV, I1) = R0;
    R0 = EMUCLK;
    DM(DK_STATS_W_T0, I1) = R0;
    R0 = DM(DK_STATS_PRIV, I1);
    RTS;

_dk_span_end:
    DM(DK_STATS_PRIV, I1) = R0;
    DM(DK_STATS_PRIV + 1, I1) = R1;
    DM(DK_STATS_PRIV + 2, I1) = R2;
    R0 = EMUCLK;
    R1 = DM(DK_STATS_W_T0, I1);
    R1 = R0 - R1;                         // the cycles of this span
    DK_SPAN_ROW_CODE
    R0 = LSHIFT R2 BY 4;
    R2 = LSHIFT R2 BY 3;
    R0 = R0 + R2;                         // 24 bytes a row
    R2 = 16;                              // DK_STATS_HEAD_WORDS words
    R0 = R0 + R2;
    R0 = R0 + R3;
    I1 = R0;                              // the row
    R0 = DM(DK_STATS_ROW_CALLS, I1);
    R0 = R0 + 1;
    DM(DK_STATS_ROW_CALLS, I1) = R0;
    R0 = DM(DK_STATS_ROW_CYCLES, I1);
    R0 = R0 + R1;
    DM(DK_STATS_ROW_CYCLES, I1) = R0;
    R0 = DM(DK_STATS_ROW_CYCLES_HI, I1);
    R0 = R0 + CI;                         // the carry of the add above (loads do not change it)
    DM(DK_STATS_ROW_CYCLES_HI, I1) = R0;
    R0 = DM(DK_STATS_ROW_LARGEST, I1);
    R0 = MAX(R0, R1);
    DM(DK_STATS_ROW_LARGEST, I1) = R0;
    R0 = DM(DK_STATS_ROW_SUM, I1);
    R0 = R0 + R1;
    DM(DK_STATS_ROW_SUM, I1) = R0;
    R3 = DM(DK_SPAN_PTR_WORD);
    I1 = R3;
    R0 = DM(DK_STATS_PRIV, I1);
    R1 = DM(DK_STATS_PRIV + 1, I1);
    R2 = DM(DK_STATS_PRIV + 2, I1);
    RTS;
