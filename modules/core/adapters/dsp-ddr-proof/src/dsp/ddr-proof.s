// Isolated DN2 1.12 quiet diagnostic. Same byte-address receive/ACK and hook
// save convention as core/dsp-link. DDR stores explicitly use normal-word DM.
// Every payload range and checksum is checked before the first destination store.
.SECTION/PM seg_pmco;
.GLOBAL _dd_hook;
.EXTERN _dd_back;
_dd_hook:
    DM(_dd_sv0) = R0;
    DM(_dd_sv3) = R3;
    DM(_dd_sv4) = R4;
    DM(_dd_sv5) = R5;
    DM(_dd_sv6) = R6;
    R0 = M5;
    DM(_dd_svm5) = R0;
    R0 = M6;
    DM(_dd_svm6) = R0;
    R0 = L2;
    DM(_dd_svl2) = R0;
    R0 = L3;
    DM(_dd_svl3) = R0;
    M5 = 0;
    M6 = 1;
    L2 = 0;
    L3 = 0;
    R0 = DM(-3, I6);
    I3 = R0;
    DM(_dd_ring) = R0;
    NOP; NOP; NOP;
    R3 = DM(I3, M6);
    R4 = 4;
    COMP(R3, R4);
    IF NE JUMP _dd_normal;
    R3 = DM(I3, M6);
    R4 = 0xDDF4A601;
    COMP(R3, R4);
    IF NE JUMP _dd_ack;
    R3 = DM(_dd_status);
    R4 = 2;
    COMP(R3, R4);
    IF GE JUMP _dd_ack; // poison persists even before token adoption
    R10 = DM(I3, M6); // exact token
    R11 = DM(I3, M6); // sequence
    R13 = DM(I3, M6); // operation
    R14 = DM(I3, M6); // destination offset in words
    R15 = DM(I3, M6); // count in words
    R0 = DM(I3, M6);
    DM(_dd_check) = R0;
    R3 = DM(_dd_token);
    R3 = PASS R3;
    IF EQ JUMP _dd_initial;
    COMP(R10, R3);
    IF NE JUMP _dd_ack; // never adopt another token
    DM(_dd_ackseq) = R11;
    R3 = DM(_dd_status);
    R4 = 2;
    COMP(R3, R4);
    IF GE JUMP _dd_ack; // sticky failure
    JUMP _dd_bounds;
_dd_initial:
    R10 = PASS R10;
    IF EQ JUMP _dd_protocol;
    R11 = PASS R11;
    IF NE JUMP _dd_protocol;
    R3 = 1;
    COMP(R13, R3);
    IF NE JUMP _dd_protocol;
    R3 = R14 OR R15;
    IF NE JUMP _dd_protocol;
_dd_bounds:
    R15 = PASS R15;
    IF LT JUMP _dd_range;
    R3 = 640;
    COMP(R15, R3);
    IF GT JUMP _dd_range;
    R14 = PASS R14;
    IF LT JUMP _dd_range;
    R3 = 240000;
    COMP(R14, R3);
    IF GT JUMP _dd_range;
    R3 = R3 - R14;
    COMP(R15, R3);
    IF GT JUMP _dd_range;
    // Check header words 0..6, then exactly count payload words.
    R0 = DM(_dd_ring);
    I3 = R0;
    R3 = 0;
    R4 = 0;
    NOP; NOP;
    LCNTR = 7, DO _dd_hsum UNTIL LCE;
        R0 = DM(I3, M6);
        R3 = R0 + R3;
        NOP;
_dd_hsum: R4 = R3 + R4;
    R0 = DM(_dd_ring);
    R5 = 128;
    R0 = R0 + R5;
    I3 = R0;
    DM(_dd_payload) = R0;
    R15 = PASS R15;
    IF EQ JUMP _dd_sumdone;
    NOP;
    LCNTR = R15, DO _dd_psum UNTIL LCE;
        R0 = DM(I3, M6);
        R3 = R0 + R3;
        NOP;
_dd_psum: R4 = R3 + R4;
_dd_sumdone:
    R3 = R3 XOR R4;
    R4 = DM(_dd_check);
    COMP(R3, R4);
    IF NE JUMP _dd_badcheck;
    R3 = DM(_dd_token);
    R3 = PASS R3;
    IF EQ JUMP _dd_init;
    // A duplicate may only repeat the immediately preceding accepted frame.
    R3 = DM(_dd_seq);
    COMP(R11, R3);
    IF NE JUMP _dd_next;
    R3 = DM(_dd_lastcheck);
    COMP(R3, R4);
    IF NE JUMP _dd_protocol;
    JUMP _dd_ack;
_dd_next:
    R3 = R3 + 1;
    COMP(R11, R3);
    IF NE JUMP _dd_protocol;
    R3 = DM(_dd_ready);
    R3 = PASS R3;
    IF NE JUMP _dd_protocol;
    R3 = DM(_dd_words);
    COMP(R14, R3);
    IF NE JUMP _dd_range;
    R3 = 2;
    COMP(R13, R3);
    IF NE JUMP _dd_commit;
    R15 = PASS R15;
    IF EQ JUMP _dd_range;
    R3 = 375;
    COMP(R11, R3);
    IF GT JUMP _dd_protocol;
    R3 = 0x10200000;
    R0 = R3 + R14;
    I2 = R0;
    R0 = DM(_dd_payload);
    I3 = R0;
    NOP; NOP; NOP; NOP;
    LCNTR = R15, DO _dd_copy UNTIL LCE;
        R0 = DM(I3, M6);
        NOP; NOP;
_dd_copy: DM(I2, M6) = R0 (NW);
    R3 = DM(_dd_words);
    R3 = R3 + R15;
    DM(_dd_words) = R3;
    JUMP _dd_accepted;
_dd_commit:
    R3 = 3;
    COMP(R13, R3);
    IF NE JUMP _dd_protocol;
    R3 = 376;
    COMP(R11, R3);
    IF NE JUMP _dd_protocol;
    R15 = PASS R15;
    IF NE JUMP _dd_range;
    R3 = 240000;
    COMP(R14, R3);
    IF NE JUMP _dd_range;
    R3 = 1;
    DM(_dd_status) = R3;
    JUMP _dd_accepted;
_dd_init:
    // This path is reachable only once: the token is published after both bands.
    I2 = 0x101ffc00;
    R0 = 0xA55A3CC3;
    NOP; NOP; NOP; NOP;
    LCNTR = 1024, DO _dd_lower UNTIL LCE;
        NOP; NOP; NOP;
_dd_lower: DM(I2, M6) = R0 (NW);
    I2 = 0x1023a980;
    NOP; NOP; NOP; NOP;
    LCNTR = 1024, DO _dd_upper UNTIL LCE;
        NOP; NOP; NOP;
_dd_upper: DM(I2, M6) = R0 (NW);
    R3 = 0;
    DM(_dd_ready) = R3;
    DM(_dd_status) = R3;
    DM(_dd_words) = R3;
    DM(_dd_token) = R10;
_dd_accepted:
    DM(_dd_seq) = R11;
    DM(_dd_ackseq) = R11;
    R3 = DM(_dd_check);
    DM(_dd_lastcheck) = R3;
    R3 = DM(_dd_status);
    R4 = 1;
    COMP(R3, R4);
    IF NE JUMP _dd_ack;
    DM(_dd_ready) = R4; // READY is published last, only by complete COMMIT
    JUMP _dd_ack;
_dd_badcheck:
    R3 = 2;
    JUMP _dd_error;
_dd_range:
    R3 = 3;
    JUMP _dd_error;
_dd_protocol:
    R3 = 4;
_dd_error:
    DM(_dd_status) = R3;
    R3 = 0;
    DM(_dd_ready) = R3;
    JUMP _dd_ack;
_dd_normal:
    // The stock dispatch reads unsigned SHORT16; the adjacent high half holds
    // pitch after note-on. Only this observer masks it. The proof gate above
    // still requires the exact complete word 4 and separate proof magic.
    R3 = FEXT R3 BY 0:16;
    R4 = 3;
    COMP(R3, R4);
    IF NE JUMP _dd_ack;
    R3 = DM(_dd_normal_frames);
    R3 = R3 + 1;
    DM(_dd_normal_frames) = R3;
_dd_ack:
    R3 = DM(_dd_token);
    R3 = PASS R3;
    IF EQ JUMP _dd_out;
    R4 = I4;
    R5 = 0x60;
    R4 = R4 + R5;
    I2 = R4;
    R4 = DM(_dd_ackseq);
    R4 = LSHIFT R4 BY 16;
    R5 = 0xA6DD;
    R4 = R4 OR R5;
    R5 = DM(_dd_status);
    R5 = LSHIFT R5 BY 25;
    R4 = R4 OR R5;
    DM(I2, M6) = R4;
    DM(I2, M6) = R3;
_dd_out:
    R0 = DM(_dd_svm5);
    M5 = R0;
    R0 = DM(_dd_svm6);
    M6 = R0;
    R0 = DM(_dd_svl2);
    L2 = R0;
    R0 = DM(_dd_svl3);
    L3 = R0;
    R0 = DM(_dd_sv0);
    R3 = DM(_dd_sv3);
    R4 = DM(_dd_sv4);
    R5 = DM(_dd_sv5);
    R6 = DM(_dd_sv6);
    JUMP _dd_back;

.SECTION/DM seg_dmda;
.GLOBAL _dd_token;
.VAR _dd_token = 0;
.GLOBAL _dd_seq;
.VAR _dd_seq = 0;
.GLOBAL _dd_ackseq;
.VAR _dd_ackseq = 0;
.GLOBAL _dd_status;
.VAR _dd_status = 0;
.GLOBAL _dd_words;
.VAR _dd_words = 0;
.GLOBAL _dd_ready;
.VAR _dd_ready = 0;
.GLOBAL _dd_check;
.VAR _dd_check = 0;
.GLOBAL _dd_lastcheck;
.VAR _dd_lastcheck = 0;
.GLOBAL _dd_ring;
.VAR _dd_ring = 0;
.GLOBAL _dd_payload;
.VAR _dd_payload = 0;
.GLOBAL _dd_sv0;
.VAR _dd_sv0 = 0;
.GLOBAL _dd_sv3;
.VAR _dd_sv3 = 0;
.GLOBAL _dd_sv4;
.VAR _dd_sv4 = 0;
.GLOBAL _dd_sv5;
.VAR _dd_sv5 = 0;
.GLOBAL _dd_sv6;
.VAR _dd_sv6 = 0;
.GLOBAL _dd_svm5;
.VAR _dd_svm5 = 0;
.GLOBAL _dd_svm6;
.VAR _dd_svm6 = 0;
.GLOBAL _dd_svl2;
.VAR _dd_svl2 = 0;
.GLOBAL _dd_svl3;
.VAR _dd_svl3 = 0;

.GLOBAL _dd_normal_frames;
.VAR _dd_normal_frames = 0;
