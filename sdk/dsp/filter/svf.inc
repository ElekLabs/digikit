// digikit DSP kernels: one band of a TPT (Zavalishin / Cytomic) state-variable filter, bandpass with unity peak gain.
//
// _dk_svf_bp (C: dk_svf_bp.)   void dk_svf_bp(int band, int args, int unused)   convention: dsp/contract/call.inc
//   maths:         with g = tan(pi f / fs), k = 1 / Q, a1 = 1 / (1 + g (g + k)), a2 = g a1, a3 = g a2, per sample and with the
//                  state (ic1, ic2) carried between samples and blocks:
//                    v3 = x - ic2
//                    v1 = a1 ic1 + a2 v3                  bandpass of the unnormalised form; its peak gain is Q
//                    v2 = ic2 + a2 ic1 + a3 v3            lowpass
//                    ic1 = 2 v1 - ic1,  ic2 = 2 v2 - ic2
//                    out = out + k v1                     unity peak gain (the centre gain is 1 for every Q), added to the block
//                  The loop uses the algebraically reduced form with the record's coefficients:
//                    y   = ka1 ic1 + ka2 v3               y = k v1,        ka1 = k a1,  ka2 = k a2
//                    ic1 = e1 ic1 + a2d v3                = 2 v1 - ic1,    e1 = 2 a1 - 1, a2d = 2 a2
//                    t   = a2d ic1 + a3d v3
//                    ic2 = ic2 + t                        = 2 v2 - ic2,    a3d = 2 a3
//                    out = out + y
//                  six multiplies and six adds per sample (the direct form needs eleven adds). The first multiply of each pair
//                  uses the ic1 and ic2 of the same sample (before they are updated). Operation order (one float32 rounding
//                  each): v3 = x - ic2; p1 = ka1 ic1; p2 = a2d ic1; p3 = e1 ic1; q2 = a3d v3; q3 = a2d v3; q1 = ka2 v3;
//                  ic1' = p3 + q3; t = p2 + q2; ic2' = ic2 + t; y = p1 + q1; out' = out + y.
//   arguments:     R4 = band record, R8 = args block (2 words: input block, output block), R12 unused;
//                  all normal-word DM addresses
//                  band record, 8 words: ic1, ic2, ka1, e1, a2d, ka2, a3d, unused (ic1 and ic2 are read at entry and written
//                  back at exit; the coefficients are only read; the bank scales ka1 and ka2 by its output gain)
//                  The input block and the output block must not overlap; the routine reads x[0] .. x[31] and reads and
//                  writes out[0] .. out[31] (accumulate mode: the caller clears the block for the first band).
//   block:         DK_BLOCK (32) samples; the sample count is fixed by the loop schedule (peeled first and last sample).
//   registers:     see _dk_svf_band below (the shared loop). Saves R3, R5, R6, R7, R9, R10, R11, R13, R14, R15, I0, I1, I5
//                  (13 slots, S = 13).
//   instructions:  252 with DK_CALL, measured (208 in the band loop, see _dk_svf_band, the rest the frame, the three loads of the
//                  arguments, CALL and the saves of 13 registers).
//
// DK_SVF_COEFS(KA1, E1, A2D, KA2, A3D, G, K, KS, T1, T2, CR, CF)
//   maths:         the record's coefficients from g, k and ks = k times the band's output gain (ks = k for unity peak gain):
//                  d = 1 + g (g + k), a1 = 1 / d, a2d = 2 g a1, a3d = g a2d, ka1 = ks a1, ka2 = ka1 g, e1 = 2 a1 - 1
//                  Operation order: d = g (g + k) + 1 (two roundings); a1 = DK_RECIP; a2d = g a1, a2d = a2d + a2d;
//                  a3d = g a2d; ka1 = ks a1; ka2 = ka1 g; e1 = a1 + a1, e1 = e1 - 1.
//   arguments:     KA1, E1, A2D, KA2, A3D = the five coefficient results; G = g, K = k and KS = ks (not changed); T1 and T2 =
//                  temporaries; CR and CF = one more temporary named by its R and F name (1.0 and 2.0 are loaded into it);
//                  all float registers distinct (E1 holds a1 until the end)
//   range:         g > 0 and k > 0; DK_RECIP needs 1 + g (g + k) in a normal range (no check)
//   instructions:  20
//   accuracy:      the reciprocal's, relative 2^-21 on a1; e1 = 2 a1 - 1 keeps |e1| < 1 (no worse than a1's rounding)
//
// _dk_svf_band: the band loop, shared by _dk_svf_bp and _dk_rbank. Not a block routine of the C convention: it is entered with
//   CALL and left with RTS, and takes its operands in fixed registers.
//   in:            I1 = band record (8 words), I4 = input block, I0 = output block (written), I5 = output block (read: the
//                  caller sets I5 to the block that holds the sum so far; for the first band of a bank it points to a block
//                  of zeros), M5 = 0, M6 = 1
//   out:           the record's ic1 and ic2 written back, I4, I0 and I5 advanced by 32; I1 is unchanged
//   clobbers:      F0 .. F15, I4, I0, I5, ASTAT, the loop registers
//   registers:     F0 ka1, F1 e1, F2 a2d, F5 ka2, F6 a2d (a second copy: the multiplier takes its operands from F0 .. F3 and
//                  F4 .. F7), F7 a3d, F3 v3, F4 ic1, F12 ic2, F8 p3, F9 p2, F10 p1, F13 q3 (and the output sample read from I5),
//                  F14 q2, F15 q1, F11 x, then y, then the output sample, then t. The ALU operands of a multifunction
//                  instruction are in F8 .. F11 and F12 .. F15. Multiplier operands are F2 x F4 .. F7 (products of ic1: F0,
//                  F1, F2 with F4) and F3 x F5 .. F7 (products of v3).
//   loop schedule: 6 instructions per sample, one multiply and one add in each (6 multiplies and 6 adds, so 6 is the floor),
//                  software pipelined over the samples: the products of ic1 of sample n + 1 start in slot 5 of sample n; the
//                  tail of sample n (y, the output sample, its store) runs in slots 1 to 3 of sample n + 1:
//                    slot 0: p2 = a2d ic1;           v3 = x - ic2
//                    slot 1: p1 = ka1 ic1;           y = p1 + q1 (of the sample before);  load out[n-1]
//                    slot 2: q3 = a2d v3;            out = y + out[n-1]
//                    slot 3: q2 = a3d v3;            ic1 = p3 + q3;  store out[n-1]
//                    slot 4: q1 = ka2 v3;            t = p2 + q2
//                    slot 5: p3 = e1 ic1 (next);     ic2 = t + ic2;  load x[n+1]
//                  Per sample the recurrence ic2 -> v3 -> q2 -> t -> ic2 is slots 0, 2, 3, 4, 5 (4 results in 6 slots); no
//                  multiplier input comes from the previous instruction's result, and no DAG register is used within 4
//                  instructions of its load in the loop. Sample 0 (no tail) and sample 31 (no next sample) are written out
//                  and the loop runs for samples 1 to 30. Counted: 10 before sample 0, 6 for sample 0, 1 for the loop count,
//                  180 in the loop, 6 for sample 31, 3 for the tail, 1 + 1 for the state stores and RTS = 208.
//   Manual:        Processing Elements, "Multifunction Computations", "Software Pipelining for Multifunction Instructions".
//
// Needs "dsp/contract/call.inc", DK_BLOCK (dk_machine.inc or dk_fx.inc) and, for DK_SVF_COEFS, "dsp/util/recip.inc" included
// before this file.

#define DK_SVF_COEFS(KA1, E1, A2D, KA2, A3D, G, K, KS, T1, T2, CR, CF) \
    T1 = G + K; \
    T1 = G * T1; \
    CR = 0x3f800000; \
    T1 = T1 + CF; \
    CR = 0x40000000; \
    DK_RECIP(E1, T1, CF, T2); \
    A2D = G * E1; \
    A2D = A2D + A2D; \
    A3D = G * A2D; \
    KA1 = KS * E1; \
    KA2 = KA1 * G; \
    E1 = E1 + E1; \
    CR = 0x3f800000; \
    E1 = E1 - CF

.SECTION/PM seg_pmco;
.GLOBAL _dk_svf_bp;
.GLOBAL dk_svf_bp.;
.GLOBAL _dk_svf_band;

_dk_svf_band:
    F0 = DM(2, I1);                    // ka1
    F1 = DM(3, I1);                    // e1
    F2 = DM(4, I1);                    // a2d
    F5 = DM(5, I1);                    // ka2
    F7 = DM(6, I1);                    // a3d
    F6 = DM(4, I1);                    // a2d, second copy
    F4 = DM(0, I1);                    // ic1
    F12 = DM(1, I1);                   // ic2
    F11 = DM(I4, M6);                  // prime: x[0]
    F8 = F1 * F4;                      // p3 of sample 0
    // sample 0: slots 0 to 5 without the tail of a sample before
    F9 = F2 * F4, F3 = F11 - F12;
    F10 = F0 * F4;
    F13 = F3 * F6;
    F14 = F3 * F7, F4 = F8 + F13;
    F15 = F3 * F5, F11 = F9 + F14;
    F8 = F1 * F4, F12 = F11 + F12, F11 = DM(I4, M6);
    // samples 1 to 30: slot n of sample j follows slot n - 1; the tail of sample j - 1 is in slots 1 to 3
    LCNTR = DK_BLOCK - 2, DO _dk_svf_band_end UNTIL LCE;
        F9 = F2 * F4, F3 = F11 - F12;                                // slot 0: p2; v3 = x - ic2
        F10 = F0 * F4, F11 = F10 + F15, F13 = DM(I5, M6);            // slot 1: p1; y = p1 + q1 (before); load out
        F13 = F3 * F6, F11 = F11 + F13;                              // slot 2: q3; out = y + out
        F14 = F3 * F7, F4 = F8 + F13, DM(I0, M6) = F11;              // slot 3: q2; ic1 = p3 + q3; store out
        F15 = F3 * F5, F11 = F9 + F14;                               // slot 4: q1; t = p2 + q2
_dk_svf_band_end: F8 = F1 * F4, F12 = F11 + F12, F11 = DM(I4, M6);   // slot 5: p3 (next); ic2 = t + ic2; load x
    // sample 31: no products for a next sample, no load of x[32]
    F9 = F2 * F4, F3 = F11 - F12;
    F10 = F0 * F4, F11 = F10 + F15, F13 = DM(I5, M6);
    F13 = F3 * F6, F11 = F11 + F13;
    F14 = F3 * F7, F4 = F8 + F13, DM(I0, M6) = F11;
    F15 = F3 * F5, F11 = F9 + F14;
    F12 = F11 + F12;
    // the tail of sample 31
    F11 = F10 + F15, F13 = DM(I5, M6);
    F11 = F11 + F13, DM(1, I1) = F12;
    DM(I0, M6) = F11;
    DM(0, I1) = F4;
    RTS;

_dk_svf_bp:
dk_svf_bp.:
    DK_ENTER(13);
    DK_SAVE(2, R3);
    DK_SAVE(3, R5);
    DK_SAVE(4, R6);
    DK_SAVE(5, R7);
    R3 = I0;
    DK_SAVE(6, R3);
    R3 = I1;
    DK_SAVE(7, R3);
    R3 = I5;
    DK_SAVE(8, R3);
    I4 = R8;                           // args block
    I1 = R4;                           // band record
    DK_SAVE(9, R9);
    DK_SAVE(10, R10);
    DK_SAVE(11, R11);
    DK_SAVE(12, R13);
    DK_SAVE(13, R14);
    DK_SAVE(14, R15);
    R0 = DM(0, I4);                    // input block
    R1 = DM(1, I4);                    // output block
    I4 = R0;
    I0 = R1;
    I5 = R1;                           // accumulate: read the output block
    CALL _dk_svf_band;
    DK_RETADDR;
    DK_RESTORE(R3, 2);
    DK_RESTORE(R5, 3);
    DK_RESTORE(R6, 4);
    DK_RESTORE(R7, 5);
    DK_RESTORE(I0, 6);
    DK_RESTORE(I1, 7);
    DK_RESTORE(I5, 8);
    DK_RESTORE(R9, 9);
    DK_RESTORE(R10, 10);
    DK_RESTORE(R11, 11);
    DK_RESTORE(R13, 12);
    DK_RESTORE(R14, 13);
    DK_LEAVE(R15, 14);
