// bus += g_k * the track's bus, g_k = F4 + (k + 1) * F5, k = 0 to 31, both channels. The track is at `_fxs_c + 8`. Keeps I4, I5, R3. The loop is software
// pipelined in four instructions a sample: the right channel is added in the same iteration, the store of the left sum goes one iteration later (the first one
// lands on the pad word before the bus) and the last one after the loop.
_fxs_mac:
    R0 = DM(_fxs_c + 8);
    I1 = R0;                              // the track, left
    R1 = FXS_RIGHT_BYTES;
    R0 = R0 + R1;
    I0 = R0;                              // the track, right
    R0 = _fxs_bus;
    R1 = 4;
    R0 = R0 + R1;
    I13 = R0;                             // the bus, left (read)
    R1 = FXS_RIGHT_BYTES;
    R2 = R0 + R1;
    I12 = R2;                             // the bus, right (read)
    I2 = R2;                              // the bus, right (write)
    I3 = _fxs_bus;                        // the pad, then the left channel (write)
    LCNTR = 32, DO _fxs_ml UNTIL LCE;
        F4 = F4 + F5, R1 = DM(I0, M6), R8 = PM(I12, M14);
        F12 = F1 * F4, R2 = DM(I1, M6), R10 = PM(I13, M14);
        F13 = F2 * F4, F8 = F8 + F12, DM(I3, M6) = R9;
_fxs_ml: F9 = F10 + F13, DM(I2, M6) = R8;
    DM(I3, M6) = R9;
    RTS;

// Adds the effect's output times its MIX to the FX return bus: return[k] += wet[k] * w_k * mix, w_k = 1 (steady), (k + 1) / 32 (fade in) or 1 - (k + 1) / 32
// (fade out), the same weights as the master blend of fx.s. Mix 0 adds nothing. The row is at `_fx_c + 0`, the mode at `_fx_c + 4`.
_fxs_ret:
    R0 = DM(_fx_c + 0);
    I4 = R0;
    R9 = DM(FX_MIX, I4);
    R0 = 0x7fffffff;
    R0 = R9 AND R0;
    IF EQ RTS;
    R5 = DM(_fx_c + 4);
    R5 = PASS R5;
    IF EQ JUMP _fxs_steady;
    R6 = 0x3d000000;                      // +1/32
    R7 = 0;                               // the weight before the first sample
    R0 = 1;
    COMP(R5, R0);
    IF EQ JUMP _fxs_rw;
    R6 = 0xbd000000;                      // -1/32
    R7 = 0x3f800000;                      // 1.0
_fxs_rw:
    I1 = _fx_scratch;
    I0 = _fxs_return;
    CALL _fxs_rchan;                      // left
    CALL _fxs_rchan;                      // right
    RTS;
_fxs_steady:                              // the weight is 1.0: return[k] += wet[k] * mix, both channels in one loop (the same products as the weighted loop)
    I1 = _fx_scratch;
    I2 = _fxs_return;
    I12 = _fxs_return;
    R4 = PASS R9;
    LCNTR = DK_FX_BLOCK, DO _fxs_sl UNTIL LCE;
        R0 = DM(I1, M6), R10 = PM(I12, M14);
        F12 = F0 * F4;
        F10 = F10 + F12;
_fxs_sl: DM(I2, M6) = R10;
    RTS;

// One channel: R6 = the weight step, R7 = the weight before the first sample, R9 = the mix, I1 = the wet block (read), I0 = the return (read and write). Each
// pointer advances by 32 words. Clobbers R0, R1, R2, R10, R11, I2.
_fxs_rchan:
    R10 = R7;
    I2 = I0;
    LCNTR = DK_BLOCK, DO _fxs_rl UNTIL LCE;
        F10 = F10 + F6;
        F11 = F10 * F9;
        R0 = DM(I1, M6);
        F1 = F0 * F11;
        R2 = DM(I2, M5);
        F2 = F2 + F1;
_fxs_rl: DM(I2, M6) = R2;
    I0 = I2;
    RTS;
