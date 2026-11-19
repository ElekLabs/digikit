// digikit DSP kernels: 2^x (polynomial on the fractional part, exponent put in by SCALB).
//
// DK_EXP2(OUT, IN1, NI, T, CR, CF)
//   maths:         n = round(x) (FIX, round to nearest: MODE1 TRUNCATE is off in a module), r = x - n, |r| <= 0.5,
//                  y = 2^r by the degree-5 minimax polynomial in r (relative error), y = SCALB(y, n)
//                    p(r) = c0 + r (c1 + r (c2 + r (c3 + r (c4 + r c5))))
//                    c0 1.0000001  c1 0.69314694  c2 0.24022120  c3 0.05550713  c4 0.00967554  c5 0.00132765
//                  The coefficients come from a relative-error Remez fit on [-0.5, 0.5] (polynomial error 7.5e-8).
//   arguments:     OUT = y (OUT must not be IN1), IN1 = x, NI = integer temporary (an R register), T = float temporary,
//                  CR and CF = one more temporary, named by its R and by its F name (the polynomial constants are loaded into
//                  it, so the macro needs no constant register); OUT, IN1, T, CF and NI must be distinct registers
//   range:         |x| up to 100 (the result must be a normal float32: SCALB adds n to the exponent without a check).
//                  The accuracy is measured over -30 .. +30 in sdk/tests/dsp/rbank.rig.test.ts.
//   instructions:  20 (3 for the argument, 16 for the polynomial with its constant loads, 1 for SCALB)
//   accuracy:      relative error at most 2.3e-7 (7.5e-8 polynomial, the rest float32 roundings: the Horner steps and the
//                  constant c0, which rounds to 1 + 2^-23); measured 2.24e-7 over -30 .. 30 in sdk/tests/dsp/rbank.rig.test.ts
//   registers:     writes OUT, NI, T, CR/CF, and ASTAT; the constants are inline immediates (the exception that the noise
//                  macros make in the same way: a polynomial has too many constants for registers)
//
// The caller writes the trailing ";".

#define DK_EXP2(OUT, IN1, NI, T, CR, CF) \
    NI = FIX IN1; \
    T = FLOAT NI; \
    T = IN1 - T; \
    CR = 0x3aae0473; \
    OUT = CF * T; \
    CR = 0x3c1e8629; \
    OUT = OUT + CF; \
    OUT = OUT * T; \
    CR = 0x3d635b72; \
    OUT = OUT + CF; \
    OUT = OUT * T; \
    CR = 0x3e75fc8c; \
    OUT = OUT + CF; \
    OUT = OUT * T; \
    CR = 0x3f317214; \
    OUT = OUT + CF; \
    OUT = OUT * T; \
    CR = 0x3f800001; \
    OUT = OUT + CF; \
    OUT = SCALB OUT BY NI
