// Private oscillator and quadrature pair adapted from digikit's Frequency shifter effect.
// The two fourth-order all-pass chains run in z^-2. A one-sample delay on
// chain P gives a near-quadrature pair across the audio band. Positive phase
// increments in P*cos + Q*sin shift frequencies up.

// Write 32 cosine/sine pairs at I4, ramping R8 by R10 and advancing phase R9 before each pair.
_asc_make_osc:
    R11 = 0x40000000;                     // a quarter turn
    R12 = 0x80000000;                     // half a turn
    R13 = 0x30800000;                     // 2^-30
    R15 = 0xbb8f3abb;
    R7 = 0x3da2d13d;
    R6 = 0xbf255b67;
    R5 = 0x3fc90fd1;
    LCNTR = 32, DO _asc_osc UNTIL LCE;
        R8 = R8 + R10;
        R9 = R9 + R8;
        R0 = PASS R9;
        R1 = R0 + R0;
        R1 = R1 XOR R0;
        IF LT R0 = R12 - R0;
        F1 = FLOAT R0;
        F1 = F1 * F13;
        F2 = F1 * F1;
        F3 = F15 * F2;
        F3 = F3 + F7;
        F3 = F3 * F2;
        F3 = F3 + F6;
        F3 = F3 * F2;
        F3 = F3 + F5;
        F4 = F3 * F1;                     // sin
        R0 = R9 + R11;
        R1 = R0 + R0;
        R1 = R1 XOR R0;
        IF LT R0 = R12 - R0;
        F1 = FLOAT R0;
        F1 = F1 * F13;
        F2 = F1 * F1;
        F3 = F15 * F2;
        F3 = F3 + F7;
        F3 = F3 * F2;
        F3 = F3 + F6;
        F3 = F3 * F2;
        F3 = F3 + F5;
        F3 = F3 * F1;                     // cos
        DM(I4, M6) = R3;
_asc_osc: DM(I4, M6) = R4;
    RTS;

// Chain P over one channel (32 samples): I0 = the input (advances 32), I1 = where the outputs go (advances 32), I3 = the 8 state words of this
// chain and channel (section k, even samples: word 2k; odd samples: word 2k + 1). Clobbers R0-R15.
_asc_pass_p:
    R0 = 0x3ee83a77;                      // c0
    R1 = 0x3f5b9207;                      // c1
    R2 = 0x3f787e57;                      // c2
    R3 = 0x3f7f281b;                      // c3
    R4 = DM(0, I3);
    R8 = DM(1, I3);
    R5 = DM(2, I3);
    R9 = DM(3, I3);
    R6 = DM(4, I3);
    R10 = DM(5, I3);
    R7 = DM(6, I3);
    R11 = DM(7, I3);
    LCNTR = 16, DO _asc_pp UNTIL LCE;
        R12 = DM(I0, M6);
        F14 = F0 * F12;        // section 0: t = c x
        F13 = F14 + F4;        // y = t + s
        F14 = F0 * F13;        // u = c y
        F4 = F14 - F12;        // s' = u - x
        F14 = F1 * F13;        // section 1: t = c x
        F12 = F14 + F5;        // y = t + s
        F14 = F1 * F12;        // u = c y
        F5 = F14 - F13;        // s' = u - x
        F14 = F2 * F12;        // section 2: t = c x
        F13 = F14 + F6;        // y = t + s
        F14 = F2 * F13;        // u = c y
        F6 = F14 - F12;        // s' = u - x
        F14 = F3 * F13;        // section 3: t = c x
        F12 = F14 + F7;        // y = t + s
        F14 = F3 * F12;        // u = c y
        F7 = F14 - F13;        // s' = u - x
        DM(I1, M6) = R12;
        R12 = DM(I0, M6);
        F14 = F0 * F12;        // section 0: t = c x
        F13 = F14 + F8;        // y = t + s
        F14 = F0 * F13;        // u = c y
        F8 = F14 - F12;        // s' = u - x
        F14 = F1 * F13;        // section 1: t = c x
        F12 = F14 + F9;        // y = t + s
        F14 = F1 * F12;        // u = c y
        F9 = F14 - F13;        // s' = u - x
        F14 = F2 * F12;        // section 2: t = c x
        F13 = F14 + F10;        // y = t + s
        F14 = F2 * F13;        // u = c y
        F10 = F14 - F12;        // s' = u - x
        F14 = F3 * F13;        // section 3: t = c x
        F12 = F14 + F11;        // y = t + s
        F14 = F3 * F12;        // u = c y
        F11 = F14 - F13;        // s' = u - x
_asc_pp: DM(I1, M6) = R12;
    DM(0, I3) = R4;
    DM(1, I3) = R8;
    DM(2, I3) = R5;
    DM(3, I3) = R9;
    DM(4, I3) = R6;
    DM(5, I3) = R10;
    DM(6, I3) = R7;
    DM(7, I3) = R11;
    RTS;

// Chain Q over one channel with the mix: I0 = the input (advances 32), I1 = the output (advances 32), I2 = the delayed chain P output (advances 32),
// I3 = the 8 state words, I4 = the oscillator (cos, sin per sample, advances 64). out = a cos + b sin. Clobbers R0-R15.
_asc_pass_q:
    R0 = 0x3e19a1c8;                      // c0
    R1 = 0x3f350f25;                      // c1
    R2 = 0x3f6f273d;                      // c2
    R3 = 0x3f7ce391;                      // c3
    R4 = DM(0, I3);
    R8 = DM(1, I3);
    R5 = DM(2, I3);
    R9 = DM(3, I3);
    R6 = DM(4, I3);
    R10 = DM(5, I3);
    R7 = DM(6, I3);
    R11 = DM(7, I3);
    LCNTR = 16, DO _asc_pq UNTIL LCE;
        R12 = DM(I0, M6);
        F14 = F0 * F12;        // section 0: t = c x
        F13 = F14 + F4;        // y = t + s
        F14 = F0 * F13;        // u = c y
        F4 = F14 - F12;        // s' = u - x
        F14 = F1 * F13;        // section 1: t = c x
        F12 = F14 + F5;        // y = t + s
        F14 = F1 * F12;        // u = c y
        F5 = F14 - F13;        // s' = u - x
        F14 = F2 * F12;        // section 2: t = c x
        F13 = F14 + F6;        // y = t + s
        F14 = F2 * F13;        // u = c y
        F6 = F14 - F12;        // s' = u - x
        F14 = F3 * F13;        // section 3: t = c x
        F12 = F14 + F7;        // y = t + s
        F14 = F3 * F12;        // u = c y
        F7 = F14 - F13;        // s' = u - x
        R13 = DM(I2, M6);                 // a (the delayed chain P output)
        R14 = DM(I4, M6);                 // cos
        F13 = F13 * F14;
        R14 = DM(I4, M6);                 // sin
        F12 = F12 * F14;
        F12 = F12 + F13;
        DM(I1, M6) = R12;
        R12 = DM(I0, M6);
        F14 = F0 * F12;        // section 0: t = c x
        F13 = F14 + F8;        // y = t + s
        F14 = F0 * F13;        // u = c y
        F8 = F14 - F12;        // s' = u - x
        F14 = F1 * F13;        // section 1: t = c x
        F12 = F14 + F9;        // y = t + s
        F14 = F1 * F12;        // u = c y
        F9 = F14 - F13;        // s' = u - x
        F14 = F2 * F12;        // section 2: t = c x
        F13 = F14 + F10;        // y = t + s
        F14 = F2 * F13;        // u = c y
        F10 = F14 - F12;        // s' = u - x
        F14 = F3 * F13;        // section 3: t = c x
        F12 = F14 + F11;        // y = t + s
        F14 = F3 * F12;        // u = c y
        F11 = F14 - F13;        // s' = u - x
        R13 = DM(I2, M6);                 // a (the delayed chain P output)
        R14 = DM(I4, M6);                 // cos
        F13 = F13 * F14;
        R14 = DM(I4, M6);                 // sin
        F12 = F12 * F14;
        F12 = F12 + F13;
_asc_pq: DM(I1, M6) = R12;
    DM(0, I3) = R4;
    DM(1, I3) = R8;
    DM(2, I3) = R5;
    DM(3, I3) = R9;
    DM(4, I3) = R6;
    DM(5, I3) = R10;
    DM(6, I3) = R7;
    DM(7, I3) = R11;
    RTS;
