// digikit DSP kernels: tan(pi x) for the prewarp of a trapezoidal filter, x = f / fs in [0, 0.45].
//
// DK_TAN(OUT, FN, A, B, D, CR, CF)
//   maths:         h = x pi / 2 (h <= 0.7069), u = h h, t = h P(u), tan(pi x) = tan(2 h) = 2 t / (1 - t t)
//                  P(u) = tan(h) / h, degree 5 in u (relative-error Remez fit on [0, 0.4997]; fit error 5.4e-8):
//                    P(u) = c0 + u (c1 + u (c2 + u (c3 + u (c4 + u c5))))
//                    c0 0.99999994  c1 0.33334103  c2 0.13315710  c3 0.05542377  c4 0.01657945  c5 0.01716015
//                  1 / (1 - t t) is DK_RECIP (t <= 0.855, so 1 - t t >= 0.268 and the divide is well conditioned).
//   arguments:     OUT = tan(pi x), FN = x (not changed), A, B, D = float temporaries, CR and CF = one more temporary
//                  named by its R and F name (constants are loaded into it); all registers distinct
//   range:         x in [0, 0.45]; above it the result is wrong (the polynomial leaves its fit interval, and at x = 0.5 the
//                  divide is by zero). The caller clamps (the bank mutes a band above 0.45 fs).
//   instructions:  33 (2 for h, 1 for u, 16 for P with its constant loads, 1 for t, 1 for t t, 2 for 1 - t t, 1 for the 2.0,
//                  7 for the reciprocal, 2 for 2 t and the product)
//   accuracy:      relative error at most 1.2e-6 over (0, 0.45] (the fit error is amplified up to 6.4 times by the divide, the
//                  reciprocal is accurate to about 2^-21); measured 1.15e-6 in sdk/tests/dsp/rbank.rig.test.ts (the rig's
//                  reciprocal seed table is the core's)
//   registers:     writes OUT, A, B, D, CR/CF and ASTAT; the constants are inline immediates (see exp2.inc)
//
// Needs "dsp/util/recip.inc" included before this file. The caller writes the trailing ";".

#define DK_TAN(OUT, FN, A, B, D, CR, CF) \
    CR = 0x3fc90fdb; \
    A = FN * CF; \
    B = A * A; \
    CR = 0x3c8c9370; \
    D = CF * B; \
    CR = 0x3c87d1a0; \
    D = D + CF; \
    D = D * B; \
    CR = 0x3d630409; \
    D = D + CF; \
    D = D * B; \
    CR = 0x3e085a56; \
    D = D + CF; \
    D = D * B; \
    CR = 0x3eaaabad; \
    D = D + CF; \
    D = D * B; \
    CR = 0x3f7fffff; \
    D = D + CF; \
    D = A * D; \
    B = D * D; \
    CR = 0x3f800000; \
    B = CF - B; \
    CR = 0x40000000; \
    DK_RECIP(A, B, CF, OUT); \
    D = D + D; \
    OUT = D * A
