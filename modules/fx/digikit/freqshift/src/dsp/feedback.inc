// Feedback state follows the shifter's first 236 words. Next are the request,
// gain, byte cursor, primed flag, four filter states and selected input pointer.
// The 64-word planar scratch does not alias the adapter's dry input. The ring
// holds 165 stereo chunks of 16 frames, interleaved, for 2640 frames at 48 kHz.
// Processing halves keeps every loop linear, even when a block straddles wrap.
#define FS_FB_GAIN 237
#define FS_FB_POS 238
#define FS_FB_PRIMED 239
#define FS_FB_LP_L 240
#define FS_FB_DC_L 241
#define FS_FB_LP_R 242
#define FS_FB_DC_R 243
#define FS_FB_TEMP_B 980
#define FS_FB_RING_B 1236
#define FS_FB_CHUNK_BYTES 128
#define FS_FB_LAST 20992
#define FS_FB_RING_BYTES 21120
#define FS_STATE_BYTES 22356

// I0 enters as the dry input and leaves as the input to both allpass chains.
// Ramp gain over the block. The first complete wrap makes the ring readable.
// If it occurs between halves, only the second half has a delayed signal.
_fs_fb_prepare:
    R0 = DM(FS_FB_TARGET, I5);
    R1 = 0;
    R2 = 0x3f4ccccd;                     // 0.8, including out-of-range requests
    R0 = MAX(R0, R1);
    R0 = MIN(R0, R2);
    R8 = DM(FS_FB_GAIN, I5);
    DM(FS_FB_GAIN, I5) = R0;
    R1 = R8 OR R0;
    IF EQ RTS;
    R3 = DM(FS_FB_POS, I5);
    R4 = DM(FS_FB_PRIMED, I5);
    R4 = PASS R4;
    IF NE JUMP _fs_fb_mix;
    R1 = FS_FB_LAST;
    COMP(R3, R1);
    IF NE RTS;

_fs_fb_mix:
    F9 = F0 - F8;
    R1 = 0x3d000000;                     // 1/32
    F9 = F9 * F1;
    R0 = I0;
    R1 = FS_FB_CHUNK_BYTES;              // offset of dry R (32 floats)
    R0 = R0 + R1;
    I1 = R0;
    R0 = I5;
    R1 = FS_FB_TEMP_B;
    R0 = R0 + R1;
    I4 = R0;
    R1 = FS_FB_CHUNK_BYTES;
    R0 = R0 + R1;
    I3 = R0;
    R0 = I5;
    R1 = FS_FB_RING_B;
    R10 = R0 + R1;
    R0 = R10 + R3;
    I2 = R0;
    R4 = PASS R4;
    IF NE JUMP _fs_fb_first;
    // At the first wrap, do not read invalid feedback for these 16 frames.
    LCNTR = 16, DO _fs_fb_copy UNTIL LCE;
        F8 = F8 + F9;
        R0 = DM(I0, M6);
        R1 = DM(I1, M6);
        DM(I4, M6) = R0;
_fs_fb_copy: DM(I3, M6) = R1;
    JUMP _fs_fb_second;

_fs_fb_first:
    CALL _fs_fb_mix_half;
_fs_fb_second:
    R1 = FS_FB_CHUNK_BYTES;
    R3 = R3 + R1;
    R1 = FS_FB_RING_BYTES;
    COMP(R3, R1);
    IF EQ R3 = R3 - R1;
    R0 = R10 + R3;
    I2 = R0;
    CALL _fs_fb_mix_half;
    R0 = I5;
    R1 = FS_FB_TEMP_B;
    R0 = R0 + R1;
    I0 = R0;
    RTS;

_fs_fb_mix_half:
    LCNTR = 16, DO _fs_fb_mix_end UNTIL LCE;
        F8 = F8 + F9;
        R0 = DM(I0, M6);
        R1 = DM(I1, M6);
        R6 = DM(I2, M6);
        R7 = DM(I2, M6);
        F6 = F6 * F8;
        F7 = F7 * F8;
        F0 = F0 + F6;
        F1 = F1 + F7;
        DM(I4, M6) = R0;
_fs_fb_mix_end: DM(I3, M6) = R1;
    RTS;

// The ring contains only the filtered wet output, before the adapter's MIX.
// On turn-off the last mixed block ramped to zero. Invalidate the ring instead
// of sweeping it again, so a subsequent turn-on cannot replay an old tail.
_fs_fb_finish:
    R0 = DM(FS_FB_GAIN, I5);
    R0 = PASS R0;
    IF NE JUMP _fs_fb_write;
    R1 = DM(FS_FB_POS, I5);
    R2 = DM(FS_FB_PRIMED, I5);
    R1 = R1 OR R2;
    IF EQ RTS;
    DM(FS_FB_POS, I5) = R0;
    DM(FS_FB_PRIMED, I5) = R0;
    DM(FS_FB_LP_L, I5) = R0;
    DM(FS_FB_DC_L, I5) = R0;
    DM(FS_FB_LP_R, I5) = R0;
    DM(FS_FB_DC_R, I5) = R0;
    RTS;

_fs_fb_write:
    I1 = DM(FS_FXIN, I5);
    R5 = FS_FB_CHUNK_BYTES;
    R0 = I5;
    R1 = FS_FB_RING_B;
    R14 = R0 + R1;
    R3 = DM(FS_FB_POS, I5);
    R0 = DM(DK_FX_IN_W_OUT, I1);
    I3 = R0;
    R0 = R0 + R5;
    I4 = R0;
    R0 = R14 + R3;
    I2 = R0;
    R2 = FS_FB_RING_BYTES;
    R8 = DM(FS_FB_LP_L, I5);
    R9 = DM(FS_FB_DC_L, I5);
    R10 = DM(FS_FB_LP_R, I5);
    R11 = DM(FS_FB_DC_R, I5);
    R12 = 0x3f0e4bc1;                    // 1 - exp(-2 pi 6200 / 48000)
    R13 = 0x3c0acfc3;                    // 1 - exp(-2 pi 65 / 48000)
    CALL _fs_fb_write_half;
    CALL _fs_fb_advance;
    R0 = R14 + R3;
    I2 = R0;
    CALL _fs_fb_write_half;
    CALL _fs_fb_advance;
    DM(FS_FB_POS, I5) = R3;
    DM(FS_FB_LP_L, I5) = R8;
    DM(FS_FB_DC_L, I5) = R9;
    DM(FS_FB_LP_R, I5) = R10;
    DM(FS_FB_DC_R, I5) = R11;
    RTS;

_fs_fb_advance:
    R3 = R3 + R5;
    COMP(R3, R2);
    IF LT RTS;
    R3 = R3 - R2;
    R0 = 1;
    DM(FS_FB_PRIMED, I5) = R0;
    RTS;

_fs_fb_write_half:
    LCNTR = 16, DO _fs_fb_write_end UNTIL LCE;
        R0 = DM(I3, M6);
        R1 = DM(I4, M6);
        F0 = F0 - F8;
        F1 = F1 - F10;
        F0 = F0 * F12;
        F1 = F1 * F12;
        F8 = F8 + F0;
        F10 = F10 + F1;
        F0 = F8 - F9;
        F1 = F10 - F11;
        F0 = F0 * F13;
        F1 = F1 * F13;
        F9 = F9 + F0;
        F11 = F11 + F1;
        F0 = F8 - F9;
        F1 = F10 - F11;
        DM(I2, M6) = R0;
_fs_fb_write_end: DM(I2, M6) = R1;
    RTS;
