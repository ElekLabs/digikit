// test/cal-kernels: five synthetic machines with known instruction mixes, for the calibration profile `kernels`
// (devices/calibration/profiles/kernels.toml). A calibration run fits device and emulator factors per kind of instruction from the
// instructions-per-call counts of these kernels, so each call executes a fixed number of instructions that does not depend on the
// input, the parameters, the state or the note. Every kernel writes silence to the output block (mix: a signal below 1e-6) and returns
// DK_OK: a voice never ends by itself, so the calibration case presses the note once and exactly one call per block follows for as long as
// the case runs. A voice that a later case leaves behind goes with the module's restart: the profile `kernels` puts a module-free case
// between two kernel cases. Silence is a loop of 8 iterations of 4 stores (33 instructions with the loop set-up). The counts below are
// instructions executed in one render call, from the first instruction to the JUMP back to the adapter (the JUMP counted); the rig test
// of the module asserts
// them (tests/kernels.rig.test.ts).
//
//   kernel  entry       kind of instruction                      loop iterations x body   instructions per render call
//   alu     _ka_render  multiply-accumulate and ALU, registers   2500 x 8                 20,050
//   dm      _kd_render  loads and stores, state buffer (1 KiB)   76 x 262                 19,959
//   mem     _km_render  streaming loads, 75 KiB bss buffer       2400 x 8                 19,244
//   branch  _kb_render  data-dependent jumps, short loops        625 x 32                 20,046
//   mix     _kx_render  oscillator and 44 biquad sections        32 x 623                 19,956
//
// The breakdown of each count is stated in front of its kernel. Memory: the module's state (_*_desc state_bytes, reserved behind the image
// by the adapter) and its .PKBSS buffer both come from the run-time DSP area of the device (free L2 SRAM, 512 KiB on the Digitakt II
// and 320 KiB on the Digitone II, shared by all modules); whether the L1 data cache covers that range is not known [O], so dm (1 KiB,
// re-read 76 times) and mem (75 KiB, read once per block in address order) differ in working set, not in the memory behind them. The
// SDRAM of the sample pool is out of scope (it needs the sample pool).
//
// Style: docs/sharc/style.md. Registers: sdk/dsp/REGISTERS.md (R0-R15, I0-I5, M1-M4, MR, LCNTR are free; M5, M6 and M7 are never
// written). Loops have 4 or more instructions; a register that feeds a DAG is loaded 4 or more instructions before it is used.

#include "dk_machine.inc"

#define KA_ITERS 2500
#define KD_OUTER 76
#define KD_INNER 32
#define KM_ITERS 2400
#define KM_BYTES 76800
#define KB_OUTER 625
#define KX_SECTIONS 44

.SECTION/PM seg_pmco;
.GLOBAL _ka_render;
.GLOBAL _ka_note_on;
.GLOBAL _kd_render;
.GLOBAL _kd_note_on;
.GLOBAL _km_render;
.GLOBAL _km_note_on;
.GLOBAL _kb_render;
.GLOBAL _kb_note_on;
.GLOBAL _kx_render;
.GLOBAL _kx_note_on;

// alu: multiply-accumulate and ALU operations on registers only, no memory access in the loop.
// Count: 14 (set-up) + 1 (LCNTR/DO) + 2500 x 8 + 33 (silence) + 2 (R0 = DK_OK, JUMP) = 20,050.
// The loop body has one MAC, three float operations, two shifts or logic and one integer add, all on distinct registers (no stalls).
_ka_render:
    I1 = R8;                              // the voice input
    R0 = DM(DK_IN_W_OUT, I1);
    R4 = 3;
    R5 = 5;
    R9 = 7;
    R10 = 11;
    R12 = 0x3f000000;                     // 0.5
    R13 = 0x3e800000;                     // 0.25
    R2 = 0;
    I0 = R0;                              // the output block
    MRF = 0;
    R3 = 0;
    R7 = 0;
    R14 = 0;
    LCNTR = KA_ITERS, DO _ka_loop UNTIL LCE;
        MRF = MRF + R4 * R5 (SSF);        // multiply-accumulate
        R8 = R9 + R10;                    // ALU
        F11 = F12 * F13;                  // float multiply
        R14 = R9 AND R10;
        F0 = F12 + F13;
        R3 = LSHIFT R9 BY 3;
        F6 = F12 - F13;
_ka_loop: R7 = R10 XOR R9;
    LCNTR = 8, DO _ka_zero UNTIL LCE;
        DM(I0, M6) = R2;
        DM(I0, M6) = R2;
        DM(I0, M6) = R2;
_ka_zero: DM(I0, M6) = R2;
    R0 = DK_OK;
    JUMP (M13, I13);

// dm: loads and stores to the voice's state buffer (256 words, 1 KiB): the outer loop points I1 at words 0 to 127 and I2 at words
// 128 to 255, the inner loop copies 4 words per iteration (4 loads, 4 stores), 32 iterations.
// Count: 11 (set-up) + 1 (LCNTR/DO) + 76 x (4 + 1 + 32 x 8 + 1) + 33 (silence) + 2 = 76 x 262 + 47 = 19,959.
// Outer iteration: I1 = R4, I2 = R5, two NOPs (the pointers are loaded 4 instructions before the first access), DO, 256 inner, one
// closing NOP (the last instruction of the outer loop is not the end of the inner loop).
_kd_render:
    I1 = R8;
    R0 = DM(DK_IN_W_OUT, I1);
    R6 = 128;
    R5 = R4 + R6;                         // the second half of the state
    R10 = 0;
    I0 = R0;
    R0 = 0;
    R1 = 0;
    R2 = 0;
    R3 = 0;
    NOP;
    LCNTR = KD_OUTER, DO _kd_outer UNTIL LCE;
        I1 = R4;
        I2 = R5;
        NOP;
        NOP;
        LCNTR = KD_INNER, DO _kd_inner UNTIL LCE;
            R0 = DM(I1, M6);
            R1 = DM(I1, M6);
            R2 = DM(I1, M6);
            R3 = DM(I1, M6);
            DM(I2, M6) = R0;
            DM(I2, M6) = R1;
            DM(I2, M6) = R2;
_kd_inner:  DM(I2, M6) = R3;
_kd_outer: NOP;
    LCNTR = 8, DO _kd_zero UNTIL LCE;
        DM(I0, M6) = R10;
        DM(I0, M6) = R10;
        DM(I0, M6) = R10;
_kd_zero: DM(I0, M6) = R10;
    R0 = DK_OK;
    JUMP (M13, I13);

// mem: streaming reads of a 75 KiB buffer (19,200 words, bss of the module's own object, so in the run-time DSP area) from its first
// word to its last, 8 loads per iteration, no use of the values. The buffer is larger than the L1 data cache would hold, so every
// block reads it from the start.
// Count: 8 (set-up) + 1 (LCNTR/DO) + 2400 x 8 + 33 (silence) + 2 = 19,244.
.PKBSS _km_buf, KM_BYTES;
_km_render:
    I1 = R8;
    R0 = DM(DK_IN_W_OUT, I1);
    R10 = 0;
    I0 = R0;
    R1 = _km_buf;
    I1 = R1;
    NOP;
    NOP;
    LCNTR = KM_ITERS, DO _km_loop UNTIL LCE;
        R0 = DM(I1, M6);
        R1 = DM(I1, M6);
        R2 = DM(I1, M6);
        R3 = DM(I1, M6);
        R4 = DM(I1, M6);
        R5 = DM(I1, M6);
        R6 = DM(I1, M6);
_km_loop: R7 = DM(I1, M6);
    LCNTR = 8, DO _km_zero UNTIL LCE;
        DM(I0, M6) = R10;
        DM(I0, M6) = R10;
        DM(I0, M6) = R10;
_km_zero: DM(I0, M6) = R10;
    R0 = DK_OK;
    JUMP (M13, I13);

// branch: an xorshift32 generator (state word 0, seeded by note_on, carried from block to block) steers three two-way branches per
// iteration, then a 3-iteration loop of 4 instructions (loop set-up and exit). Both paths of a branch execute 4 instructions (test,
// conditional jump, one operation, and a jump or a NOP), so the count does not depend on the bits; the taken and not-taken jumps differ.
// Iteration: 6 (xorshift) + 3 x 4 (branches) + 1 (LCNTR/DO) + 3 x 4 (loop) + 1 (closing instruction, outside the inner loop) = 32.
// Count: 9 (set-up) + 1 (LCNTR/DO) + 625 x 32 + 1 (state store) + 33 (silence) + 2 = 20,046.
_kb_render:
    I1 = R8;
    R0 = DM(DK_IN_W_OUT, I1);
    I2 = R4;
    R10 = 0;
    I0 = R0;
    R9 = 3;
    R3 = 0;
    R4 = 0;
    R8 = DM(0, I2);
    LCNTR = KB_OUTER, DO _kb_outer UNTIL LCE;
        R0 = LSHIFT R8 BY 13;
        R1 = R8 XOR R0;
        R0 = LSHIFT R1 BY -17;
        R2 = R1 XOR R0;
        R0 = LSHIFT R2 BY 5;
        R8 = R2 XOR R0;
        BTST R8 BY 0;
        IF SZ JUMP _kb_b1;
        R3 = R4 + R9;
        JUMP _kb_j1;
_kb_b1: R3 = R4 - R9;
        NOP;
_kb_j1: BTST R8 BY 5;
        IF SZ JUMP _kb_b2;
        R3 = R4 + R9;
        JUMP _kb_j2;
_kb_b2: R3 = R4 - R9;
        NOP;
_kb_j2: BTST R8 BY 11;
        IF SZ JUMP _kb_b3;
        R3 = R4 + R9;
        JUMP _kb_j3;
_kb_b3: R3 = R4 - R9;
        NOP;
_kb_j3: LCNTR = 3, DO _kb_inner UNTIL LCE;
            R4 = R3 + R9;
            R5 = R4 XOR R3;
            R6 = R5 AND R4;
_kb_inner:  R7 = R6 OR R5;
_kb_outer: R12 = R7 + R3;
    DM(0, I2) = R8;
    LCNTR = 8, DO _kb_zero UNTIL LCE;
        DM(I0, M6) = R10;
        DM(I0, M6) = R10;
        DM(I0, M6) = R10;
_kb_zero: DM(I0, M6) = R10;
    R0 = DK_OK;
    JUMP (M13, I13);

// mix: a blend like a typical voice: a sawtooth oscillator (phase += 0.01, wrapped at 1) feeds a cascade of 44 transposed direct form
// II biquad sections (5 multiplies, 5 adds, 2 loads, 2 stores and a pass: 14 instructions), the coefficients in registers and the two
// state words of each section in the state buffer. Output: the last section's output, which is below 1e-6 (each section has a gain below 1).
// State (356 bytes, cleared at note-on): word 0 the phase, words 1 to 88 the section states.
// Per sample: 1 (I1 = I2) + 4 (oscillator) + 1 (LCNTR/DO) + 44 x 14 + 1 (store) = 623.
// Count: 16 (set-up) + 1 (LCNTR/DO) + 32 x 623 + 1 (phase store) + 2 = 19,956. (No silence loop: the sample loop writes the 32 samples.)
_kx_render:
    I1 = R8;
    R0 = DM(DK_IN_W_OUT, I1);
    I3 = R4;
    R1 = 1;
    R2 = R4 + R1;                         // the section states start at word 1
    I0 = R0;
    I2 = R2;
    R7 = DM(0, I3);                       // phase
    R4 = 0x3e800000;                      // b0 = 0.25
    R5 = 0x3e800000;                      // b1 = 0.25
    R6 = 0x3d800000;                      // b2 = 0.0625
    R10 = 0x3e4ccccd;                     // -a1 = 0.2
    R11 = 0xbd4ccccd;                     // -a2 = -0.05
    R8 = 0x3c23d70a;                      // phase increment 0.01
    R14 = 0x3f800000;                     // 1.0
    R15 = 0x3f000000;                     // 0.5
    LCNTR = DK_BLOCK, DO _kx_sample UNTIL LCE;
        I1 = I2;
        F7 = F7 + F8;
        F2 = F7 - F14;
        IF GE F7 = PASS F2;
        F12 = F7 - F15;
        LCNTR = KX_SECTIONS, DO _kx_sec UNTIL LCE;
            F1 = DM(0, I1);
            F2 = DM(1, I1);
            F3 = F4 * F12;
            F0 = F3 + F1;
            F3 = F5 * F12;
            F9 = F10 * F0;
            F3 = F3 + F9;
            F1 = F3 + F2;
            F3 = F6 * F12;
            F12 = PASS F0;
            F9 = F11 * F0;
            F13 = F3 + F9;
            DM(I1, M6) = F1;
_kx_sec:    DM(I1, M6) = F13;
_kx_sample: DM(I0, M6) = F12;
    DM(0, I3) = R7;
    R0 = DK_OK;
    JUMP (M13, I13);

// note_on of every kernel does nothing except branch, which seeds its xorshift state. None is at offset 0 of the image.
_ka_note_on:
    R0 = 0;
    JUMP (M13, I13);
_kd_note_on:
    R0 = 0;
    JUMP (M13, I13);
_km_note_on:
    R0 = 0;
    JUMP (M13, I13);
_kb_note_on:
    I1 = R4;
    R0 = 0x2545f491;
    DM(0, I1) = R0;                       // the generator seed
    R0 = 0;
    JUMP (M13, I13);
_kx_note_on:
    R0 = 0;
    JUMP (M13, I13);

.SECTION/DM seg_dmda;
// dk_machine_desc: magic, contract, n_params, voices, state_bytes, state_clear_bytes, max_call_depth, stack_bytes.
.GLOBAL _ka_desc;
.VAR _ka_desc = DK_MAGIC;
.VAR _ka_desc1 = DK_MACHINE_V1;
.VAR _ka_desc2 = 0;
.VAR _ka_desc3 = 1;
.VAR _ka_desc4 = 4;
.VAR _ka_desc5 = 4;
.VAR _ka_desc6 = 2;
.VAR _ka_desc7 = 0;
.GLOBAL _kd_desc;
.VAR _kd_desc = DK_MAGIC;
.VAR _kd_desc1 = DK_MACHINE_V1;
.VAR _kd_desc2 = 0;
.VAR _kd_desc3 = 1;
.VAR _kd_desc4 = 1024;
.VAR _kd_desc5 = 1024;
.VAR _kd_desc6 = 2;
.VAR _kd_desc7 = 0;
.GLOBAL _km_desc;
.VAR _km_desc = DK_MAGIC;
.VAR _km_desc1 = DK_MACHINE_V1;
.VAR _km_desc2 = 0;
.VAR _km_desc3 = 1;
.VAR _km_desc4 = 4;
.VAR _km_desc5 = 4;
.VAR _km_desc6 = 2;
.VAR _km_desc7 = 0;
.GLOBAL _kb_desc;
.VAR _kb_desc = DK_MAGIC;
.VAR _kb_desc1 = DK_MACHINE_V1;
.VAR _kb_desc2 = 0;
.VAR _kb_desc3 = 1;
.VAR _kb_desc4 = 4;
.VAR _kb_desc5 = 4;
.VAR _kb_desc6 = 2;
.VAR _kb_desc7 = 0;
.GLOBAL _kx_desc;
.VAR _kx_desc = DK_MAGIC;
.VAR _kx_desc1 = DK_MACHINE_V1;
.VAR _kx_desc2 = 0;
.VAR _kx_desc3 = 1;
.VAR _kx_desc4 = 356;
.VAR _kx_desc5 = 356;
.VAR _kx_desc6 = 2;
.VAR _kx_desc7 = 0;
