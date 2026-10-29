// Six SDK cursor words followed by a private admitted-generation token.
// Machine-adapter entry is JUMP. Preserve I12/I13 across the kernel's CJUMP ABI.
.SECTION/PM seg_pmco;
.GLOBAL _dd_play_render;
.GLOBAL _dd_play_note_on;
.EXTERN _dd_ready;
.EXTERN _dd_token;
_dd_play_render:
    // Raw context before observer arithmetic; ASTATX low 12 is retained raw.
    R1 = ASTATX;
    DM(_dd_obs_render_last_astatx) = R1;
    R1 = MODE1;
    DM(_dd_obs_render_last) = R1;
    R1 = I6;
    DM(_dd_obs_render_last_i6) = R1;
    R1 = I7;
    DM(_dd_obs_render_last_i7) = R1;
    R1 = I13;
    DM(_dd_obs_render_last_i13) = R1;
    R1 = DM(_dd_obs_active);
    DM(_dd_obs_render_last_active) = R1;
    R1 = DM(_dd_obs_render_entries);
    R1 = PASS R1;
    IF NE JUMP _dd_obs_render_seen;
    R1 = DM(_dd_obs_render_last);
    DM(_dd_obs_render_first) = R1;
    R1 = DM(_dd_obs_render_last_astatx);
    DM(_dd_obs_render_first_astatx) = R1;
    R1 = DM(_dd_obs_render_last_i6);
    DM(_dd_obs_render_first_i6) = R1;
    R1 = DM(_dd_obs_render_last_i7);
    DM(_dd_obs_render_first_i7) = R1;
    R1 = DM(_dd_obs_render_last_i13);
    DM(_dd_obs_render_first_i13) = R1;
    R1 = DM(_dd_obs_render_last_active);
    DM(_dd_obs_render_first_active) = R1;
_dd_obs_render_seen:
    R1 = DM(_dd_obs_render_entries);
    R1 = R1 + 1;
    DM(_dd_obs_render_entries) = R1;
    R1 = DM(_dd_obs_active);
    R1 = R1 + 1;
    DM(_dd_obs_active) = R1;
    // Save exact incoming mask/bank context; never enable interrupts blindly.
    R2 = MODE1;
    BIT CLR MODE1 0x1000;
    NOP; NOP;
    R1 = DM(_dd_admission_open);
    R1 = PASS R1;
    IF EQ JUMP _dd_render_denied_masked;
    R1 = DM(_dd_retire_closed);
    R1 = PASS R1;
    IF NE JUMP _dd_render_denied_masked;
    R1 = DM(_dd_admitted);
    R1 = PASS R1;
    IF LT JUMP _dd_render_fault_masked;
    R0 = 0x7fffffff;
    COMP(R1, R0);
    IF GE JUMP _dd_render_fault_masked;
    // Compare the cached generation in the same masked admission section.
    I3 = R4;
    R0 = DM(6, I3);
    R0 = PASS R0;
    IF EQ JUMP _dd_render_denied_masked;
    R1 = DM(_dd_token);
    COMP(R0, R1);
    IF NE JUMP _dd_render_denied_masked;
    R1 = DM(_dd_admitted);
    R1 = R1 + 1;
    DM(_dd_admitted) = R1;
    MODE1 = R2;
    NOP; NOP;
    JUMP _dd_render_admitted;
_dd_render_fault_masked:
    R1 = 1;
    DM(_dd_admit_fault) = R1;
    DM(_dd_retire_closed) = R1;
    R1 = 0;
    DM(_dd_admission_open) = R1;
    DM(_dd_ready) = R1;
_dd_render_denied_masked:
    MODE1 = R2;
    NOP; NOP;
    JUMP _dd_render_silent;
_dd_render_admitted:
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
    R0 = DM(_dd_play_reads);
    R0 = R0 + 1;
    DM(_dd_play_reads) = R0;
    DK_CALL(_dk_sample_pcm16_linear, _dd_play_after_linear);
    MODIFY(I7, 1) (NW);
    I12 = R3;
    I13 = R5;
    // Last possible backing read is before this guarded release.
    R2 = MODE1;
    BIT CLR MODE1 0x1000;
    NOP; NOP;
    R1 = DM(_dd_admitted);
    R1 = PASS R1;
    IF LE JUMP _dd_render_release_fault;
    R1 = R1 - 1;
    DM(_dd_admitted) = R1;
    JUMP _dd_render_released;
_dd_render_release_fault:
    R1 = 1;
    DM(_dd_admit_fault) = R1;
    DM(_dd_retire_closed) = R1;
    R1 = 0;
    DM(_dd_admission_open) = R1;
    DM(_dd_ready) = R1;
    R1 = 0x7fffffff; // never let corrupt bookkeeping claim a drain
    DM(_dd_admitted) = R1;
_dd_render_released:
    MODE1 = R2;
    NOP; NOP;
    R1 = DM(_dd_obs_active);
    R1 = R1 - 1;
    DM(_dd_obs_active) = R1;
    R1 = DM(_dd_obs_render_exits);
    R1 = R1 + 1;
    DM(_dd_obs_render_exits) = R1;
    R0 = DM(5, I3);
    DM(_dd_play_done) = R0;
    NOP; NOP; NOP;
    JUMP (M13, I13);

_dd_play_note_on:
    // Raw context before observer arithmetic; ASTATX low 12 is retained raw.
    R1 = ASTATX;
    DM(_dd_obs_note_last_astatx) = R1;
    R1 = MODE1;
    DM(_dd_obs_note_last) = R1;
    R1 = I6;
    DM(_dd_obs_note_last_i6) = R1;
    R1 = I7;
    DM(_dd_obs_note_last_i7) = R1;
    R1 = I13;
    DM(_dd_obs_note_last_i13) = R1;
    R1 = DM(_dd_obs_active);
    DM(_dd_obs_note_last_active) = R1;
    R1 = DM(_dd_obs_note_entries);
    R1 = PASS R1;
    IF NE JUMP _dd_obs_note_seen;
    R1 = DM(_dd_obs_note_last);
    DM(_dd_obs_note_first) = R1;
    R1 = DM(_dd_obs_note_last_astatx);
    DM(_dd_obs_note_first_astatx) = R1;
    R1 = DM(_dd_obs_note_last_i6);
    DM(_dd_obs_note_first_i6) = R1;
    R1 = DM(_dd_obs_note_last_i7);
    DM(_dd_obs_note_first_i7) = R1;
    R1 = DM(_dd_obs_note_last_i13);
    DM(_dd_obs_note_first_i13) = R1;
    R1 = DM(_dd_obs_note_last_active);
    DM(_dd_obs_note_first_active) = R1;
_dd_obs_note_seen:
    R1 = DM(_dd_obs_note_entries);
    R1 = R1 + 1;
    DM(_dd_obs_note_entries) = R1;
    R1 = DM(_dd_obs_active);
    R1 = R1 + 1;
    DM(_dd_obs_active) = R1;
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
    DM(6, I3) = R0;
    R0 = 1;
    DM(5, I3) = R0;
    R0 = 65536;
    DM(4, I3) = R0;
    // Save exact incoming mask/bank context; never enable interrupts blindly.
    R2 = MODE1;
    BIT CLR MODE1 0x1000;
    NOP; NOP;
    R1 = DM(_dd_admission_open);
    R1 = PASS R1;
    IF EQ JUMP _dd_note_denied_masked;
    R1 = DM(_dd_retire_closed);
    R1 = PASS R1;
    IF NE JUMP _dd_note_denied_masked;
    R1 = DM(_dd_admitted);
    R1 = PASS R1;
    IF LT JUMP _dd_note_fault_masked;
    R0 = 0x7fffffff;
    COMP(R1, R0);
    IF GE JUMP _dd_note_fault_masked;
    // Only a fresh accepted note can bind this voice to the current bank.
    R0 = DM(_dd_ready);
    R1 = 1;
    COMP(R0, R1);
    IF NE JUMP _dd_note_denied_masked;
    R0 = DM(_dd_token);
    R0 = PASS R0;
    IF EQ JUMP _dd_note_denied_masked;
    DM(6, I3) = R0;
    R1 = DM(_dd_admitted);
    R1 = R1 + 1;
    DM(_dd_admitted) = R1;
    MODE1 = R2;
    NOP; NOP;
    JUMP _dd_note_admitted;
_dd_note_fault_masked:
    R1 = 1;
    DM(_dd_admit_fault) = R1;
    DM(_dd_retire_closed) = R1;
    R1 = 0;
    DM(_dd_admission_open) = R1;
    DM(_dd_ready) = R1;
_dd_note_denied_masked:
    MODE1 = R2;
    NOP; NOP;
    JUMP _dd_play_note_observer_return;
_dd_note_admitted:
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
    // Last possible backing read is before this guarded release.
    R2 = MODE1;
    BIT CLR MODE1 0x1000;
    NOP; NOP;
    R1 = DM(_dd_admitted);
    R1 = PASS R1;
    IF LE JUMP _dd_note_release_fault;
    R1 = R1 - 1;
    DM(_dd_admitted) = R1;
    JUMP _dd_note_released;
_dd_note_release_fault:
    R1 = 1;
    DM(_dd_admit_fault) = R1;
    DM(_dd_retire_closed) = R1;
    R1 = 0;
    DM(_dd_admission_open) = R1;
    DM(_dd_ready) = R1;
    R1 = 0x7fffffff; // never let corrupt bookkeeping claim a drain
    DM(_dd_admitted) = R1;
_dd_note_released:
    MODE1 = R2;
    NOP; NOP;
_dd_play_note_observer_return:
    R1 = DM(_dd_obs_active);
    R1 = R1 - 1;
    DM(_dd_obs_active) = R1;
    R1 = DM(_dd_obs_note_exits);
    R1 = R1 + 1;
    DM(_dd_obs_note_exits) = R1;
    R0 = 0;
    JUMP (M13, I13);


// A denied render never enters the reader, including a previously armed voice.
_dd_render_silent:
    I3 = R4;
    I2 = R8;
    DM(_dd_play_state) = R4;
    R0 = 0;
    DM(0, I3) = R0;
    DM(1, I3) = R0;
    DM(2, I3) = R0;
    DM(3, I3) = R0;
    DM(4, I3) = R0;
    DM(6, I3) = R0;
    R1 = 1;
    DM(5, I3) = R1;
    DM(_dd_play_done) = R1;
    R1 = DM(DK_IN_W_OUT, I2);
    DM(_dd_play_output) = R1;
    I2 = R1;
    R2 = 32;
    NOP; NOP; NOP;
_dd_render_zero:
    DM(I2, M6) = R0;
    R2 = R2 - 1;
    R2 = PASS R2;
    IF GT JUMP _dd_render_zero;
    R1 = DM(_dd_obs_active);
    R1 = R1 - 1;
    DM(_dd_obs_active) = R1;
    R1 = DM(_dd_obs_render_exits);
    R1 = R1 + 1;
    DM(_dd_obs_render_exits) = R1;
    R0 = 1;
    JUMP (M13, I13);
