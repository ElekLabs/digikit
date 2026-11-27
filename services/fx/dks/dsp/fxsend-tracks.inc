    // the tracks: I4 = the new gains, I5 = this row's old gains
    R0 = DM(_fxs_c + 0);
    R1 = 6;
    R0 = LSHIFT R0 BY R1;                 // 16 floats a row
    R1 = _fxs_prev;
    R0 = R0 + R1;
    I5 = R0;
    I4 = _fxs_gnew;
    R0 = _fxs_trackbus;
    DM(_fxs_c + 8) = R0;                  // the track's bus
    R3 = 0x3d000000;                      // 1 / 32 (kept: _fxs_mac does not use R3)
    LCNTR = FXS_TRACKS, DO _fxs_tl UNTIL LCE;
        R1 = DM(I4, M6);                  // the new gain
        R2 = DM(I5, M5);                  // the old gain
        R0 = R1 OR R2;
        IF EQ JUMP _fxs_ts;               // both 0: the track adds nothing
        DM(I5, M6) = R1;
        F0 = F1 - F2;
        F5 = F0 * F3;                     // the step
        R4 = PASS R2;                     // the gain before the first step
        CALL _fxs_mac;
        JUMP _fxs_te;
_fxs_ts: R0 = DM(I5, M6);                 // a dummy read: its post-modify steps over the track
_fxs_te: R0 = DM(_fxs_c + 8);
        R1 = FXS_TRACK_BYTES;
        R0 = R0 + R1;
_fxs_tl: DM(_fxs_c + 8) = R0;
    RTS;
