// digikit DSP kernels: resonator bank, 8 bandpass bands (TPT state-variable filters, unity peak gain) summed.
//
// _dk_rbank (C: dk_rbank.)   void dk_rbank(int state, int args, int params)   convention: dsp/contract/call.inc
//   maths:         three knob words w in 0 .. 0x7f00 (DK_P_PERCENT, an integer) give p = clamp(w / 32512, 0, 1) (a word outside the range cannot overflow k):
//                    FREQ  p0   centre of the band 3.5 octaves below the mean: 20 Hz .. 5 kHz, logarithmic  f = 20 250^p0
//                    SPRD  p1   0 .. 2 octaves between neighbouring bands                                    s = 2 p1
//                    Q     p2   0.7 .. 50, logarithmic                                                       Q = 0.7 (50/0.7)^p2
//                  Each p is smoothed by a one-pole, p' = p' + alpha (p - p'), alpha = 1 - exp(-1/3) = 0.2835 (the 2 ms time
//                  constant at one step per block, 32 samples at 48 kHz = 0.667 ms; one step per block): the first
//                  block after the state is cleared uses the knobs as they are, with no smoothing. The smoothed values are
//                  kept in the state.
//                  Band i (0 .. 7) has the centre c_i = f 2^(s (i - 3.5)) Hz, taken as c_0 = 2^(log2 f - 3.5 s), c_(i+1) = c_i 2^s,
//                  x_i = min(max(c_i, 20 Hz), 0.45 fs) / 48000: a band above 0.45 fs sits at 0.45 fs
//                  (no mute, so no click when a centre crosses it).
//                  Per band, once per block (no ramp inside the block): g = tan(pi x_i), k = 1 / Q, and the coefficients of
//                  svf.inc (DK_SVF_COEFS); then the band's 32 samples (_dk_svf_band) add its bandpass output to the output block.
//                  Output: out = (sum of the 8 band outputs) / S. Each band has gain 1 at its own centre; S is the sum of the
//                  bands' gains at the centre of the middle band (the overlap sum, taken as magnitudes), computed once per block
//                  from the smoothed SPRD and Q: S = sum over j of 1 / sqrt(1 + Q^2 (u^d - u^-d)^2), u = 2^s, d = j - 3.
//                  The bands are symmetric, so S = 1 + 2 (t1 + t2 + t3) + t4. At SPRD 0 S = 8. The factor 1 / S is folded into
//                  the coefficients ka1 and ka2 of every band (ks = k / S), so the loop costs nothing extra. The peak gain of
//                  the bank is about 1 (below it where the bands' phases differ), except where bands pile up at a clamp (20 Hz or 0.45 fs)
//                  (several bands at the same centre add up there: 2.3 where bands 0 to 2 sit at 20 Hz, SPRD 2 octaves, FREQ 104 Hz).
//                  A setting without a clamped band has a peak gain of 0.8 to 1.0 at the centres (measured in the test).
//   arguments:     R4 = state block, R8 = args block (2 words: input block, output block), R12 = params (3 words: FREQ, SPRD, Q);
//                  all normal-word DM addresses. The input block and the output block must not overlap: the input is read
//                  once for each band. The output block is written (not accumulated): the first band reads a block of zeros
//                  in the state instead of the output block.
//   state block:   104 words (416 bytes), all 0 (or any contents with word 0 = 0) before the first block; word 0 = 0 makes
//                  the next block the first one again, which also clears the band states and the zero block.
//                      0          init flag: 0 until the first block has run, then 1
//                      1, 2, 3    smoothed FREQ, SPRD, Q (p, floats in 0 .. 1)
//                      4 .. 7     unused
//                      8 + 8 i    band i record (svf.inc): ic1, ic2, ka1, e1, a2d, ka2, a3d, unused. The coefficients are
//                                 rewritten every block.
//                      72 .. 103  32 words of zeros, read as the output block of the first band; never written after the
//                                 first block
//   saves:         R3, R5, R6, R7, R9, R10, R11, R13, R14, R15, I0, I1, I2, I3, I5 and one slot for the state pointer
//                  (16 slots, S = 16). M4 is written (M4 = 8, caller-saved). SIMD is not used: PEYEN stays clear.
//   instructions:  steady block 2489 with DK_CALL (measured in sdk/tests/dsp/rbank.rig.test.ts; the
//                  test asserts at most 2500; 8 x 208 of it is the band loop). The first block adds the clear of 96 words.
//   registers:     F0 .. F15 and R4 .. R15 as listed in the steps; I1 state or band record, I2 input block, I3 output block,
//                  I4 input pointer, I0 output pointer, I5 output read pointer, M4 = 8 (record stride).
//
// Needs "dsp/contract/call.inc", DK_BLOCK, "dsp/util/recip.inc", "dsp/util/exp2.inc", "dsp/util/tan.inc" and
// "dsp/filter/svf.inc" included before this file.

.SECTION/PM seg_pmco;
.GLOBAL _dk_rbank;
.GLOBAL dk_rbank.;

_dk_rbank:
dk_rbank.:
    DK_ENTER(16);
    DK_SAVE(2, R3);
    DK_SAVE(3, R5);
    DK_SAVE(4, R6);
    DK_SAVE(5, R7);
    DK_SAVE(6, R9);
    DK_SAVE(7, R10);
    DK_SAVE(8, R11);
    DK_SAVE(9, R13);
    DK_SAVE(10, R14);
    DK_SAVE(11, R15);
    DK_SAVE(17, R4);                   // the state pointer, read again for the second phase
    R3 = I0;
    DK_SAVE(12, R3);
    R3 = I1;
    DK_SAVE(13, R3);
    R3 = I2;
    DK_SAVE(14, R3);
    R3 = I3;
    DK_SAVE(15, R3);
    R3 = I5;
    DK_SAVE(16, R3);
    I5 = R12;                          // params
    I4 = R8;                           // args block
    I1 = R4;                           // state
    R7 = 0x38010204;                   // 1 / 32512 (0x7f00)
    R9 = 0x3f800000;                   // 1.0
    R10 = 0x3e9122cf;                  // alpha = 1 - exp(-1/3) = 0.28346869
    R0 = DM(0, I5);                    // FREQ word
    R1 = DM(1, I5);                    // SPRD word
    R2 = DM(2, I5);                    // Q word
    R3 = DM(0, I4);                    // input block
    R5 = DM(1, I4);                    // output block
    I2 = R3;
    I3 = R5;
    F0 = FLOAT R0;
    F1 = FLOAT R1;
    F2 = FLOAT R2;
    F0 = F0 * F7;
    F1 = F1 * F7;
    F2 = F2 * F7;
    F0 = MIN(F0, F9);                  // p = clamp(w / 32512, 0, 1), three knobs in F0, F1, F2
    F1 = MIN(F1, F9);
    F2 = MIN(F2, F9);
    R4 = 0;
    F0 = MAX(F0, F4);
    F1 = MAX(F1, F4);
    F2 = MAX(F2, F4);
    R3 = DM(0, I1);                    // init flag
    R3 = PASS R3;
    IF EQ JUMP _dk_rbank_first;
    // steady block: p' = p' + alpha (p - p'), one step
    F8 = DM(1, I1);
    F11 = DM(2, I1);
    F13 = DM(3, I1);
    F3 = F0 - F8;
    F5 = F1 - F11;
    F6 = F2 - F13;
    F3 = F3 * F10;
    F5 = F5 * F10;
    F6 = F6 * F10;
    F0 = F8 + F3;
    F1 = F11 + F5;
    F2 = F13 + F6;
    DM(1, I1) = F0;
    DM(2, I1) = F1;
    DM(3, I1) = F2;
_dk_rbank_join:
    // F0, F1, F2 = smoothed p0, p1, p2. The band 0 centre, the band ratio and k, by three 2^x:
    //   x_0 = log2(20 / 48000) + 7.9658 p0 - 7 p1 (c_0 / fs = 2^x_0), r = 2^(2 p1), k = 2^(-log2(0.7) - 6.1584 p2)
    R4 = 0x40fee7b4;                   // log2(250) = 7.9657841
    R5 = 0xc133a93e;                   // log2(20 / 48000) = -11.2288189
    R6 = 0x40e00000;                   // 7
    F3 = F0 * F4;
    F6 = F1 * F6;
    F3 = F3 - F6;
    F3 = F3 + F5;
    DK_EXP2(F12, F3, R8, F9, R10, F10);     // F12 = c_0 / fs
    F3 = F1 + F1;
    DK_EXP2(F13, F3, R8, F9, R10, F10);     // F13 = r = 2^s
    R4 = 0xc0c511da;                   // -log2(50 / 0.7) = -6.1584291
    R5 = 0x3f03bb11;                   // -log2(0.7) = 0.5145732
    F3 = F2 * F4;
    F3 = F3 + F5;
    DK_EXP2(F14, F3, R8, F9, R10, F10);     // F14 = k = 1 / Q
    // output gain 1 / S at the middle band: S = sum over the bands j of 1 / sqrt(1 + Q^2 (u^d - u^-d)^2), d = j - 3, u = 2^s;
    // the terms of d and -d are equal, so S = 1 + 2 (t1 + t2 + t3) + t4
    R11 = 0x40000000;                  // 2.0
    DK_RECIP(F0, F14, F11, F9);        // Q
    F0 = F0 * F0;                      // Q^2
    DK_RECIP(F1, F13, F11, F9);        // 1 / u
    R4 = 0x3f800000;                   // 1.0
    R5 = 0x3f000000;                   // 0.5
    R6 = 0x3fc00000;                   // 1.5
    R2 = R13;                          // u^d
    R3 = R1;                           // u^-d
    F7 = F2 - F3;
    F7 = F7 * F7;
    F7 = F7 * F0;
    F7 = F7 + F4;
    DK_RSQRT(F15, F7, F5, F6, F9, F10);     // t1
    F2 = F2 * F13;
    F3 = F3 * F1;
    F7 = F2 - F3;
    F7 = F7 * F7;
    F7 = F7 * F0;
    F7 = F7 + F4;
    DK_RSQRT(F8, F7, F5, F6, F9, F10);      // t2
    F15 = F15 + F8;
    F2 = F2 * F13;
    F3 = F3 * F1;
    F7 = F2 - F3;
    F7 = F7 * F7;
    F7 = F7 * F0;
    F7 = F7 + F4;
    DK_RSQRT(F8, F7, F5, F6, F9, F10);      // t3
    F15 = F15 + F8;
    F15 = F15 + F15;
    F2 = F2 * F13;
    F3 = F3 * F1;
    F7 = F2 - F3;
    F7 = F7 * F7;
    F7 = F7 * F0;
    F7 = F7 + F4;
    DK_RSQRT(F8, F7, F5, F6, F9, F10);      // t4
    F15 = F15 + F8;
    F15 = F15 + F4;                    // S
    DK_RECIP(F8, F15, F11, F9);        // 1 / S
    F15 = F14 * F8;                    // ks = k / S: the bands' unity peak gain times 1 / S
    R10 = 0x3ee66666;                  // 0.45, the highest band centre over fs
    R11 = 0x39da740e;                  // 20 / 48000, the lowest
    R0 = DM(-17, I6);                  // state pointer
    R1 = 14;
    R0 = R0 + R1;                      // band 0 record + 6: the stores below count from the last coefficient
    M4 = 8;
    I1 = R0;
    // phase 1: the coefficients of the eight bands into their records
    LCNTR = 8, DO _dk_rbank_coef_end UNTIL LCE;
        F0 = MAX(F12, F11);            // max(c_i / fs, 20 Hz / fs)
        F1 = MIN(F0, F10);             // ct = min(that, 0.45): a band above 0.45 fs sits at 0.45 fs
        DK_TAN(F2, F1, F3, F4, F5, R6, F6);                               // g
        DK_SVF_COEFS(F7, F8, F9, F0, F1, F2, F14, F15, F3, F4, R6, F6);   // ka1, e1, a2d, ka2, a3d
        F12 = F12 * F13;               // c_(i+1) / fs = r c_i / fs
        DM(-4, I1) = F7;               // ka1 (record word 2)
        DM(-3, I1) = F8;               // e1
        DM(-2, I1) = F9;               // a2d
        DM(-1, I1) = F0;               // ka2
_dk_rbank_coef_end: DM(I1, M4) = F1;   // a3d (record word 6), then to the next record
    // phase 2: the eight bands, band 0 reads the zero block, the others the sum so far
    R1 = DM(-17, I6);                  // state pointer
    R0 = 8;
    R2 = 72;
    R0 = R1 + R0;
    R2 = R1 + R2;
    I1 = R0;                           // band 0 record
    I5 = R2;                           // the zero block
    I4 = I2;
    I0 = I3;
    LCNTR = 8, DO _dk_rbank_next UNTIL LCE;
        CALL _dk_svf_band;
        MODIFY(I1, M4);                // the next record: 4 instructions before the band loop reads through I1
        I5 = I3;                       // the next bands add to the output block
        I4 = I2;
_dk_rbank_next: I0 = I3;
    DK_RETADDR;
    DK_RESTORE(R3, 2);
    DK_RESTORE(R5, 3);
    DK_RESTORE(R6, 4);
    DK_RESTORE(R7, 5);
    DK_RESTORE(R9, 6);
    DK_RESTORE(R10, 7);
    DK_RESTORE(R11, 8);
    DK_RESTORE(R13, 9);
    DK_RESTORE(R14, 10);
    DK_RESTORE(I0, 12);
    DK_RESTORE(I1, 13);
    DK_RESTORE(I2, 14);
    DK_RESTORE(I3, 15);
    DK_RESTORE(I5, 16);
    DK_LEAVE(R15, 11);

// the first block: the knobs are the smoothed values, the state is cleared
_dk_rbank_first:
    DM(1, I1) = F0;
    DM(2, I1) = F1;
    DM(3, I1) = F2;
    R5 = DM(-17, I6);                  // state pointer
    R7 = 8;
    R5 = R5 + R7;
    I4 = R5;
    R3 = 1;
    DM(0, I1) = R3;                    // init flag
    R3 = 0;
    LCNTR = 24, DO _dk_rbank_clear_end UNTIL LCE;
        DM(I4, M6) = R3;
        DM(I4, M6) = R3;
        DM(I4, M6) = R3;
_dk_rbank_clear_end: DM(I4, M6) = R3;  // the 96 words 8 .. 103: band records and the zero block
    JUMP _dk_rbank_join;
