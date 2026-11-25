// core/dsp-voice on the Digitone II: the render hook stub of the FM engine's host (docs/findings/06, "Per-voice hook on DN2"; R12-8 of docs/plans/runtime-and-services.md).
//
// The image keeps the hook site, the voice-0 heartbeat with the context words and `_gs_block`, the helper routines and all data that the dispatch code and the machine modules
// share. The code that decides what runs (the mask, the registry scan, the contract adapter, the output guard) is dispatch-dn2.s of the machine-registry owner. The stereo
// code of dsp-voice-dn2-stereo.s stays here too: it shares data with the dispatch both ways.
//
// The hook is FUN_b8251b, a 55-instruction helper the engine calls once per voice buffer after the synthesis, with R15 = 16 - v (a count that runs 16, 15, ... 1),
// R8 = R12 = the voice buffer, R4 = the voice's control block. patchkit overwrites its first instruction (`I7 = MODIFY(I7, -18)`, 48 bits) with `JUMP _dv_entry`. The helper has a second user on
// another path (a tail jump from FUN_b8258f); that call has R8 != R12 and is passed through. All registers are the engine's at the entry: the stub saves what it uses.
//
// The stub reads one slot of core/dsp-link's slot block (`_dl_slots`; sdk/dsp/contract/dk_stub.inc):
//   slot 0: no dispatch code is installed. The stub runs the displaced instruction and the stock helper, so the image renders exactly the stock audio.
//   else:   the pc of the dispatch entry. The stub saves M13 and I12 in image words, pins M13 = 0 and jumps with `JUMP (M13, I12)`. The entry restores them.
// R0 is saved in `_dv_s_r0` on every path; `_dv_idle` restores it before the displaced instruction. Voice 0 (R15 = 16, R8 = R12, the buffer's low bits 0x10 + 0x80 k) writes the heartbeat and
// the context registers as the firmware left them (finding 45, dl_diag) with absolute stores, then publishes the stereo events of the block that has ended (`_gs_block`: R0 and R1 only).

#include "adapter-dn2.inc"
#include "dk_machine.inc"
#include "dk_stub.inc"

.SECTION/PM seg_pmco;
.GLOBAL _dv_entry;
.GLOBAL _dv_idle;
.GLOBAL _dv_slots;
.EXTERN _fw_render_next;
.EXTERN _fw_frame;
.EXTERN _dl_slots;
.EXTERN _gs_block;

_dv_entry:
    DM(_dv_s_r0) = R0;
    R0 = 16;
    COMP(R0, R15);
    IF NE JUMP _dv_slot;                  // not slot 0
    COMP(R8, R12);
    IF NE JUMP _dv_slot;                  // the engine's per-voice call has R8 = R12 (the other path is passed through)
    DM(_dv_s_r1) = R1;
    R0 = 0x7f;
    R1 = R12 AND R0;
    R0 = 0x10;
    COMP(R1, R0);
    IF NE JUMP _dv_hbe;
    // once per block (slot 0): the heartbeat and the context registers as the firmware left them (finding 45, dl_diag)
    R1 = DM(_dv_diag);
    R1 = R1 + 1;
    DM(_dv_diag) = R1;
    R1 = M5;
    DM(_dv_diag + 4) = R1;
    R1 = M6;
    DM(_dv_diag + 8) = R1;
    R1 = M7;
    DM(_dv_diag + 12) = R1;
    R1 = M13;
    DM(_dv_diag + 16) = R1;
    R1 = M14;
    DM(_dv_diag + 20) = R1;
    R1 = M15;
    DM(_dv_diag + 24) = R1;
    R1 = I6;
    DM(_dv_diag + 28) = R1;
    R1 = I7;
    DM(_dv_diag + 32) = R1;
    R1 = B7;
    DM(_dv_diag + 36) = R1;
    R1 = L7;
    DM(_dv_diag + 40) = R1;
    R1 = MODE1;
    DM(_dv_diag + 44) = R1;
    CALL _gs_block;                       // publishes the stereo events of the block that has ended (dsp-voice-dn2-stereo.s); clobbers R0 and R1 only
_dv_hbe:
    R1 = DM(_dv_s_r1);
_dv_slot:
    R0 = DM(_dl_slots + DK_SLOT_VOICE_RENDER);
    R0 = PASS R0;
    IF EQ JUMP _dv_idle;
    DM(_dv_sv_m13) = M13;
    DM(_dv_sv_i12) = I12;
    M13 = 0;
    I12 = R0;
    JUMP (M13, I12);

_dv_idle:
    R0 = DM(_dv_s_r0);
    MODIFY(I7, -18) (NW);                 // the displaced first instruction of the stock helper
    JUMP _fw_render_next;

// ---- helpers for machine routines (ordinary subroutines: CALL _dv_..., return with RTS) ----

// F0 = 2 ** F0 for F0 > -64 (relative error 8e-8: a degree-5 minimax polynomial of 2^f on [0, 1) and an exact
// scale by 2^floor). Clobbers R0-R3.
.GLOBAL _dv_exp2;
_dv_exp2:
    R1 = 0x42800000;                      // 64.0
    F0 = F0 + F1;
    R1 = TRUNC F0;
    F2 = FLOAT R1;
    F0 = F0 - F2;                         // f in [0, 1)
    R2 = 0x3af61942;
    F2 = F2 * F0;
    R3 = 0x3c1347f4;
    F2 = F2 + F3;
    F2 = F2 * F0;
    R3 = 0x3d64aa27;
    F2 = F2 + F3;
    F2 = F2 * F0;
    R3 = 0x3e75ead4;
    F2 = F2 + F3;
    F2 = F2 * F0;
    R3 = 0x3f31727b;
    F2 = F2 + F3;
    F2 = F2 * F0;
    R3 = 0x3f7fffff;
    F2 = F2 + F3;
    R3 = -64;
    R1 = R3 + R1;
    F0 = SCALB F2 BY R1;
    RTS;

// R0 = the unsigned 16-bit word k of the DSP's frame copy (R0 = k, 0..4095; the frame copy is about 1,350 halfwords); _dv_frame_sw sign-extends.
// Clobbers R0-R2, I4.
.GLOBAL _dv_frame_hw;
.GLOBAL _dv_frame_sw;
_dv_frame_hw:
    R1 = 0xffe;
    R1 = R0 AND R1;
    R1 = LSHIFT R1 BY 1;
    R2 = _fw_frame;
    R1 = R2 + R1;
    I4 = R1;
    R1 = DM(0, I4);
    R2 = FEXT R0 BY 0:1;
    R2 = LSHIFT R2 BY 4;
    R0 = 0x400;                           // len6 = 16 at Ry[11:6], bit6 below it
    R2 = R2 OR R0;
    R0 = FEXT R1 BY R2;
    RTS;
_dv_frame_sw:
    R1 = 0xffe;
    R1 = R0 AND R1;
    R1 = LSHIFT R1 BY 1;
    R2 = _fw_frame;
    R1 = R2 + R1;
    I4 = R1;
    R1 = DM(0, I4);
    R2 = FEXT R0 BY 0:1;
    R2 = LSHIFT R2 BY 4;
    R0 = 0x400;                           // len6 = 16 at Ry[11:6], bit6 below it
    R2 = R2 OR R0;
    R0 = FEXT R1 BY R2;
    R0 = LSHIFT R0 BY 16;
    R1 = -16;
    R0 = ASHIFT R0 BY R1;
    RTS;

.SECTION/DM seg_dmda;
.PKDATA _dv_save, 128;                     // R1-R15, I0-I3, I5, I12, I13, M1-M4
.PKDATA _dv_s_r0, 4;
.PKDATA _dv_s_r1, 4;
.PKDATA _dv_sv_m13, 4;                     // the registers the stub pinned (dk_stub.inc)
.PKDATA _dv_sv_i12, 4;
.PKDATA _dv_s_i4, 4;
.PKDATA _dv_s_l4, 4;
.PKDATA _dv_mask, 4;                       // registered types: bit t (bit 31: 31 or more), rebuilt by slot 0 (see _dv_entry)
.PKDATA _dv_q1, 4;
.PKDATA _dv_q2, 4;
.PKDATA _dv_q4, 4;
.PKDATA _dv_ql4, 4;
.PKDATA _dv_ctx, 48;                       // M5, M6, M7, M13, M14, M15, L0-L3, L5, MODE1 of the context
// The diagnostic block (finding 45): word 0 counts the blocks that reached the hook, words 1-11 are M5, M6, M7, M13, M14, M15,
// I6, I7, B7, L7, MODE1 as the firmware left them at slot 0. core/dsp-link copies the block into the record it sends to the
// ColdFire each block (dl_diag).
.PKDATA _dv_diag, 128;
// Word 12 (the breadcrumb) holds only the call and the return stage here (_dv_mark), word 13 (the dispatch count of the Digitakt II wrapper, finding 48) stays 0; _dv_rec is written by core/dsp-link: [0] and [1] the two reply records, [2] the address of the module step word.
.PKDATA _dv_rec, 12;
.PKDATA _dv_t, 4;
.PKDATA _dv_typ, 4;
.PKDATA _dv_buf, 4;
.PKDATA _dv_row, 4;
// The contract adapter's data: the `dk_voice_in` it hands to the module, the parameter words (8), its scalars (see `_dv_contract`), the slots'
// records (4 words each) and the knob positions of the module's parameters in order (ADM_PARAM_SLOT0 .. 7; 255: none).
.PKDATA _dv_in, DK_IN_BYTES;
.PKDATA _dv_params, 32;                    // live effective UI/debug words
.PKDATA _dv_call_params, 32;               // merged callback words (latched declarations over live words)
// Adapter-private snapshots: DT2 track / DN2 physical voice slot, never firmware-owned state.
.PKDATA _dv_trig_params, 512;
.PKDATA _dv_c, 64;
.PKDATA _dv_cst, 256;
.VAR _dv_slots = ADM_PARAM_SLOT0;
.VAR _dv_slots1 = ADM_PARAM_SLOT1;
.VAR _dv_slots2 = ADM_PARAM_SLOT2;
.VAR _dv_slots3 = ADM_PARAM_SLOT3;
.VAR _dv_slots4 = ADM_PARAM_SLOT4;
.VAR _dv_slots5 = ADM_PARAM_SLOT5;
.VAR _dv_slots6 = ADM_PARAM_SLOT6;
.VAR _dv_slots7 = ADM_PARAM_SLOT7;

