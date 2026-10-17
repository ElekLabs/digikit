// test/inline-tone: a sine routine for core/dsp-inline, the hardware probe of the hook's two return modes (H1).
//
// One source, two builds: the file that comes first in the object's sources defines TONE_MODE.
//   TONE_MODE 0 (test/inline-tone-96k, image h1a): 64 samples at 96 kHz into the work buffer (I1), return with
//                `JUMP _di_return_work`; the stock decimator halves them.
//   TONE_MODE 1 (test/inline-tone-48k, image h1b): 32 samples at 48 kHz straight into the voice's output (I2),
//                return with `JUMP _di_return_out`; the decimator is skipped.
// A routine uses one continuation only (core/dsp-inline ABI): the type always returns through the same label.
//
// Per block, for voice v of track t (ABI of modules/core/dsp-inline/src/dsp/dsp-inline.s: R0 = v, R2 = t):
//   n   = pitch / 256 + (tune / 256 - 64)      pitch = frame halfword 1 + t (note << 8); tune = frame halfword
//                                             0x6d + 0x30 t (byte 0xda + 0x60 t, machine slot 0, mirror 25,
//                                             0x4000 at rest)
//   x   = (n - 69) / 12 = (pitch + tune - 34048) / 3072, at most 5.5 (19.9 kHz)
//   inc = 2^32 f / FS = C 2^(x + 12), C = 440 2^32 / FS / 4096; 2^(x + 12) = 2^k 2^r, k = trunc(x + 12) in 0..17,
//         2^r a degree-5 polynomial on [0, 1) (relative error 2e-7); the largest inc (1.8e9 at 48 kHz) fits in 31
//         bits
//   phase: one unsigned 32-bit word per voice in _it_phase; set to 0 in the block whose trigger-latch byte of the
//         voice is non-zero (a note-on), else carried over from the last block; the sum wraps modulo 2^32
//   out[k] = 0.125 sin(2 pi phase / 2^32), phase += inc. With q = phase (signed) / 2^30 in [-2, 2):
//         t = sign(q) min(|q|, 2 - |q|) in [-1, 1], sin(pi t / 2) = t (c1 + t^2 (c3 + t^2 (c5 + t^2 c7))),
//         error 1.6e-6, the coefficients scaled by the gain 0.125
//
// Registers: the helpers (_di_frame_hw, _di_trig) clobber R0-R2 and I4; v and t are kept in R8 and R9 across the
// calls. Only R0-R15, I3, I4 (and I1 or I2, the output pointer, advanced) and the ALU state change; I6, I7, M5-M7,
// M13-M15, the L registers and MODE1 are not touched. Right shifts are not used; symbol offsets are computed in
// registers (no `symbol + constant` operands).

#ifndef TONE_MODE
#error TONE_MODE is not defined: list the mode file before tone.s in the object's sources
#endif

.SECTION/PM seg_pmco;
.GLOBAL _it_render;
.EXTERN _di_frame_hw;
.EXTERN _di_trig;
.EXTERN _di_return_work;
.EXTERN _di_return_out;

// Every instruction in its normal 48-bit form: no compressed (16- or 32-bit) encodings, no indirect jumps.
.NOCOMPRESS;

_it_render:
    R8 = PASS R0;                         // v
    R9 = PASS R2;                         // t
    R0 = R9 + 1;                          // halfword 1 + t: the track's pitch, note << 8
    CALL _di_frame_hw;
    F10 = FLOAT R0;
    R0 = LSHIFT R9 BY 5;
    R1 = LSHIFT R9 BY 4;
    R0 = R0 + R1;
    R1 = 0x6d;
    R0 = R0 + R1;                         // halfword 0x6d + 0x30 t: machine slot 0, TUNE
    CALL _di_frame_hw;
    F11 = FLOAT R0;
    F10 = F10 + F11;
    R1 = 0x47050000;                      // 34048.0 = 0x4000 (TUNE at rest) + 69 * 256 (A4)
    F10 = F10 - F1;
    R1 = 0x39aaaaab;                      // 1 / 3072
    F10 = F10 * F1;                       // x = (n - 69) / 12
    R1 = 0x40b00000;                      // 5.5
    F10 = MIN(F10, F1);
    R1 = 0x41400000;                      // 12.0
    F10 = F10 + F1;                       // x + 12, at least 0.9 (pitch and tune are unsigned)
    R4 = TRUNC F10;                       // k
    F1 = FLOAT R4;
    F10 = F10 - F1;                       // r in [0, 1)
    R2 = 0x3af5cf32;                      // 2^r = a0 + r (a1 + r (a2 + r (a3 + r (a4 + r a5))))
    F2 = F2 * F10;
    R1 = 0x3c133f4c;
    F2 = F2 + F1;
    F2 = F2 * F10;
    R1 = 0x3d64b441;
    F2 = F2 + F1;
    F2 = F2 * F10;
    R1 = 0x3e75e8f6;
    F2 = F2 + F1;
    F2 = F2 * F10;
    R1 = 0x3f317297;
    F2 = F2 + F1;
    F2 = F2 * F10;
    R1 = 0x3f7ffffd;
    F2 = F2 + F1;
    R4 = LSHIFT R4 BY 23;
    R2 = R2 + R4;                         // times 2^k: k added to the exponent field
#if TONE_MODE == 1
    R1 = 0x46162fc9;                      // C = 440 2^32 / 48000 / 4096
#else
    R1 = 0x45962fc9;                      // C = 440 2^32 / 96000 / 4096
#endif
    F2 = F2 * F1;
    R6 = TRUNC F2;                        // inc

    // the voice's phase word; 0 in the block of a note-on
    R0 = PASS R8;
    CALL _di_trig;
    R1 = LSHIFT R8 BY 2;
    R2 = _it_phase;
    R1 = R1 + R2;
    I3 = R1;
    R5 = DM(I3, M5);
    R1 = 0;
    R0 = PASS R0;
    IF NE R5 = PASS R1;

    R13 = 0x30800000;                     // 2^-30
    R14 = 0x40000000;                     // 2.0
    R10 = 0x3e490fba;                     // c1 * 0.125
    R11 = 0xbda55a19;                     // c3 * 0.125
    R12 = 0x3c22be77;                     // c5 * 0.125
    R15 = 0xba0ea1ea;                     // c7 * 0.125
#if TONE_MODE == 1
    R3 = 32;
#else
    R3 = 64;
#endif
_it_loop:
    F0 = FLOAT R5;
    F0 = F0 * F13;                        // q
    F1 = ABS F0;
    F2 = F14 - F1;
    F1 = MIN(F1, F2);                     // |t|
    F0 = PASS F0;
    IF LT F1 = -F1;                       // t
    F2 = F1 * F1;
    F7 = F2 * F15;
    F7 = F7 + F12;
    F7 = F7 * F2;
    F7 = F7 + F11;
    F7 = F7 * F2;
    F7 = F7 + F10;
    F7 = F7 * F1;
#if TONE_MODE == 1
    DM(I2, M6) = R7;
#else
    DM(I1, M6) = R7;
#endif
    R5 = R5 + R6;
    R3 = R3 - 1;
    IF NE JUMP _it_loop;

    DM(I3, M5) = R5;
    // The return: a direct jump (type 8a) to the hook's continuation for this mode; no indirect jump (the routine
    // ABI in modules/core/dsp-inline/src/dsp/dsp-inline.s, docs/findings/20-patch-toolchains.md).
#if TONE_MODE == 1
    JUMP _di_return_out;
#else
    JUMP _di_return_work;
#endif

.SECTION/DM seg_dmda;
.PKDATA _it_phase, 128;                   // 32 voices, one phase word each
