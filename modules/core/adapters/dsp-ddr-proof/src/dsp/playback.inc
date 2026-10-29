// Six-word cursor, fixed immutable packed PCM16 source until reboot.
// Machine-adapter entry is JUMP. Preserve I12/I13 across the kernel's CJUMP ABI.
.SECTION/PM seg_pmco;
.GLOBAL _dd_play_render;
.GLOBAL _dd_play_note_on;
.EXTERN _dd_ready;
_dd_play_render:
    R3 = I12;
    R5 = I13;
    MODIFY(I7, -1) (NW);
    I3 = R4;
    I2 = R8;
    DM(_dd_play_state) = R4;
    R0 = DM(_dd_play_blocks);
    R0 = R0 + 1;
    DM(_dd_play_blocks) = R0;
    R4 = 0x10200000;
    R8 = DM(DK_IN_W_OUT, I2);
    DM(_dd_play_output) = R8;
    R12 = I3;
    DK_CALL(_dk_sample_pcm16_linear, _dd_play_after_linear);
    MODIFY(I7, 1) (NW);
    I12 = R3;
    I13 = R5;
    R0 = DM(5, I3);
    DM(_dd_play_done) = R0;
    NOP; NOP; NOP;
    JUMP (M13, I13);

_dd_play_note_on:
    I3 = R4;
    I2 = R12;
    DM(_dd_play_state) = R4;
    R0 = DM(_dd_play_notes);
    R0 = R0 + 1;
    DM(_dd_play_notes) = R0;
    R0 = 0;
    DM(0, I3) = R0;
    DM(1, I3) = R0;
    DM(2, I3) = R0;
    DM(3, I3) = R0;
    R0 = 1;
    DM(5, I3) = R0;
    R0 = 65536;
    DM(4, I3) = R0;
    R0 = DM(_dd_ready);
    R1 = 1;
    COMP(R0, R1);
    IF NE JUMP _dd_play_note_return;
    R0 = 480000;
    DM(1, I3) = R0;
    R0 = 0;
    DM(5, I3) = R0;
    R0 = DM(0, I2);
    R1 = 128; // Reverse >= 0.5; declared knob range 0..1 in Q8.8.
    COMP(R0, R1);
    IF LT JUMP _dd_play_note_return;
    R0 = 479999;
    DM(2, I3) = R0;
    R0 = -65536;
    DM(4, I3) = R0;
_dd_play_note_return:
    R0 = 0;
    JUMP (M13, I13);
