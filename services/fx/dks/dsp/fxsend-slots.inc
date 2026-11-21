    // The Digitone II has no track buses before the master function: the engine's 16 voice slots (a bus each, 0x100 bytes apart) are summed into the tracks inside it, by the
    // map slot -> track (16 halfwords in 8 words, slot v in half v % 2 of word v / 2, low half first; the entry of a free slot stays, its bus is silent). The send bus is the
    // sum over the slots of the gain of the slot's track times the slot's bus; a track that sounds in several slots (a chord, release tails) sums the same as one track bus.
    // The old gains are per track (16 floats a row) and are brought up to date once all slots have been mixed.
//
// The master function fades a slot out over the block when its flag byte is not 0 (16 bytes at 0x2578b8, one a slot, behind the map: the DSP sets it for one block when it gives the slot to
// a new voice on another track; the slot's bus then still holds the old voice's last block, mapped to the old track, and the map follows one block later). The master scales that
// slot's samples by 14.126 * (1 - k / 31), k = 0 to 31 (the step 0x3ee94c03 is 14.126 / 31), sample 0 unscaled. The send mix does the same with the weight 1 - k / 31 (a flagged
// slot is scaled into _fxs_fbuf, which the mix then reads in place of the bus), so a stolen voice leaves the send bus as it leaves its track.
    I1 = _fxs_slotmap;
    I0 = _fxs_trk;
    R5 = 15;
    LCNTR = 8, DO _fxs_mw UNTIL LCE;
        R0 = DM(I1, M6);
        R1 = FEXT R0 BY 0:16;
        R1 = R1 AND R5;
        DM(I0, M6) = R1;
        R1 = FEXT R0 BY 16:16;
        R1 = R1 AND R5;
_fxs_mw: DM(I0, M6) = R1;
    I1 = _fxs_fadetab;
    I0 = _fxs_flg;
    LCNTR = 4, DO _fxs_fw UNTIL LCE;      // the 16 fade flags, a byte each, into 16 words
        R0 = DM(I1, M6);
        R1 = FEXT R0 BY 0:8;
        DM(I0, M6) = R1;
        R1 = FEXT R0 BY 8:8;
        DM(I0, M6) = R1;
        R1 = FEXT R0 BY 16:8;
        DM(I0, M6) = R1;
        R1 = FEXT R0 BY 24:8;
_fxs_fw: DM(I0, M6) = R1;
    I5 = _fxs_flg;
    R0 = DM(_fxs_c + 0);
    R1 = 6;
    R0 = LSHIFT R0 BY R1;                 // 16 floats a row
    R1 = _fxs_prev;
    R0 = R0 + R1;
    DM(_fxs_c + 16) = R0;                 // this row's old gains
    I4 = _fxs_trk;
    R0 = _fxs_slotbus;
    DM(_fxs_c + 8) = R0;                  // the slot's bus
    R3 = 0x3d000000;                      // 1 / 32 (kept: _fxs_mac does not use R3)
    LCNTR = FXS_TRACKS, DO _fxs_tl UNTIL LCE;
        R0 = DM(I4, M6);                  // the slot's track
        R6 = DM(I5, M6);                  // its fade flag (I5: _fxs_mac keeps it)
        R5 = LSHIFT R0 BY 2;              // its word offset
        R0 = _fxs_gnew;
        R0 = R0 + R5;
        I1 = R0;
        R1 = DM(0, I1);                   // the new gain
        R0 = DM(_fxs_c + 16);
        R0 = R0 + R5;
        I1 = R0;
        R2 = DM(0, I1);                   // the old gain
        R0 = R1 OR R2;
        IF EQ JUMP _fxs_te;               // both 0: the slot adds nothing
        F0 = F1 - F2;
        F5 = F0 * F3;                     // the step
        R4 = PASS R2;                     // the gain before the first step
        R6 = PASS R6;
        IF EQ JUMP _fxs_tm;
        CALL _fxs_fade;                   // a fading slot: its bus scaled into _fxs_fbuf, which _fxs_c + 8 now names
        CALL _fxs_mac;
        R0 = DM(_fxs_c + 24);
        DM(_fxs_c + 8) = R0;              // the slot's own bus again
        JUMP _fxs_te;
_fxs_tm: CALL _fxs_mac;
_fxs_te: R0 = DM(_fxs_c + 8);
        R1 = FXS_TRACK_BYTES;
        R0 = R0 + R1;
_fxs_tl: DM(_fxs_c + 8) = R0;
    R0 = DM(_fxs_c + 16);
    I1 = R0;
    I0 = _fxs_gnew;
    LCNTR = FXS_TRACKS, DO _fxs_cp UNTIL LCE;
        R0 = DM(I0, M6);
_fxs_cp: DM(I1, M6) = R0;
    RTS;

// The slot bus at `_fxs_c + 8` times the fade-out weights w_k = 1 - k / 31 (w_0 = 1, w_k + 1 = w_k - 1 / 31), both channels, into _fxs_fbuf; `_fxs_c + 8` names _fxs_fbuf afterwards
// and the slot's own bus is kept at `_fxs_c + 24`. Keeps R3, R4, R5, R6, I4, I5 (the mix's gain, step, track list and flags).
_fxs_fade:
    R0 = DM(_fxs_c + 8);
    DM(_fxs_c + 24) = R0;
    I0 = R0;                              // the slot, left
    R1 = FXS_RIGHT_BYTES;
    R0 = R0 + R1;
    I1 = R0;                              // the slot, right
    I2 = _fxs_fbuf;                       // the copy, left
    R0 = _fxs_fbuf;
    R1 = FXS_RIGHT_BYTES;
    R0 = R0 + R1;
    I3 = R0;                              // the copy, right
    R7 = 0x3f800000;                      // the weight, 1.0
    R8 = 0x3d042108;                      // 1 / 31
    LCNTR = 32, DO _fxs_fl UNTIL LCE;
        R0 = DM(I0, M6);
        R1 = DM(I1, M6);
        F0 = F0 * F7;
        F1 = F1 * F7;
        F7 = F7 - F8;
        DM(I2, M6) = R0;
_fxs_fl: DM(I3, M6) = R1;
    R0 = _fxs_fbuf;
    DM(_fxs_c + 8) = R0;
    RTS;
