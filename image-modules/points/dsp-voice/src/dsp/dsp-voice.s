// core/dsp-voice, DT2: the hook stubs of the SHARC engine (docs/findings/06, "Per-voice render hook"; R12-8 of docs/plans/runtime-and-services.md).
//
// The image keeps the two hook sites and everything the dispatch code and the machine modules share: the voice registry, the diagnostic block, the save words,
// the per-track state and the helper routines. The code that decides what runs (the mask scan, the contract adapter, the output guard) is dsp-voice-dispatch.s.
//
// Each stub reads one slot of core/dsp-link's slot block (`_dl_slots`; sdk/dsp/contract/dk_stub.inc):
//   slot 0: no dispatch code is installed. The stub runs the displaced instruction and the stock path, so the image renders exactly the stock audio.
//   else:   the pc of the dispatch entry. The stub saves the register it pins in an image word, pins M13 = 0 and jumps with `JUMP (M13, I12)`. The entry restores it.
//
// Render hook. patchkit overwrites the first instruction of the render entry, `MODIFY(I7, -26) (NW)`, with `JUMP _dv_entry`. At _dv_entry the stock contract holds:
// R4 = voice record, R8 = the voice's output buffer, R12 = 32, R14 = the voice loop counter (32 on the first voice, 1 on the last), I6 = the frame the caller's CJUMP built.
// R0 and R1 are free; I12 is free (the stock path assigns it before it reads it). M13 is the context's and is not guaranteed to be 0 (docs/findings/45).
// Voice 0 (R14 = 32) writes the heartbeat and the context registers as the firmware left them (finding 45, dl_diag) with absolute stores, so that no address register is used.
//
// Unpack hook (DT2). The frame consumer's two calls of the per-track unpack (R12 = track, R8 = voice) come to _dv_unpack. R0 is free there; the stub also saves I12.

#include "adapter-dt2.inc"
#include "dk_machine.inc"
#include "dk_stub.inc"

.SECTION/PM seg_pmco;
.GLOBAL _dv_entry;
.GLOBAL _dv_unpack;
.GLOBAL _dv_idle;
.GLOBAL _dv_slots;
.EXTERN _fw_render_next;
.EXTERN _fw_unpack;
.EXTERN _fw_frame;
.EXTERN _dl_slots;

_dv_unpack:
    R0 = DM(_dl_slots + DK_SLOT_VOICE_UNPACK);
    R0 = PASS R0;
    IF EQ JUMP _fw_unpack;
    DM(_dv_sv_m13u) = M13;
    DM(_dv_sv_i12u) = I12;
    M13 = 0;
    I12 = R0;
    JUMP (M13, I12);

_dv_entry:
    R0 = 32;
    COMP(R0, R14);
    IF NE JUMP _dv_slot;
    // once per block (voice 0): the heartbeat and the context registers as the firmware left them (finding 45, dl_diag)
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
_dv_slot:
    R0 = DM(_dl_slots + DK_SLOT_VOICE_RENDER);
    R0 = PASS R0;
    IF EQ JUMP _dv_idle;
    DM(_dv_sv_m13) = M13;
    M13 = 0;
    I12 = R0;
    JUMP (M13, I12);

_dv_idle:
    MODIFY(I7, -26) (NW);                 // the displaced first instruction of the stock renderer
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
#ifdef DV_C3_DB
    RTS (DB);
    NOP;
    NOP;
#else
    RTS;
#endif

// R0 = the unsigned 16-bit word k of the DSP's frame copy (R0 = k, 0..511); _dv_frame_sw sign-extends.
// Clobbers R0-R2, I4.
.GLOBAL _dv_frame_hw;
.GLOBAL _dv_frame_sw;
_dv_frame_hw:
    R1 = 0x1fe;
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
#ifdef DV_C3_DB
    RTS (DB);
    NOP;
    NOP;
#else
    RTS;
#endif
_dv_frame_sw:
    R1 = 0x1fe;
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
#ifdef DV_C3_DB
    RTS (DB);
    NOP;
    NOP;
#else
    RTS;
#endif

.SECTION/DM seg_dmda;
.PKDATA _dv_sv_l4, 4;
.PKDATA _dv_sv_m13, 4;                     // the registers the stubs pinned (dk_stub.inc)
.PKDATA _dv_sv_m13u, 4;
.PKDATA _dv_sv_i12u, 4;
.PKDATA _dv_mask, 4;                      // registered types (see _dv_nodiag)
.PKDATA _dv_sv3, 4;
.PKDATA _dv_sv5, 4;
.PKDATA _dv_bcw, 4;
// The guard's scalars and arrays (words): 0 row end, 1 type, 2 voice, 3 track, 4 voice record, 5 output block, 6 mask of clamped knobs, 7 halfword of knob 0,
// 8 fault kind, 9 faults, 10 disabled types, 11 to 18 the originals of the clamped words, 19 to 26 the words the routine saw.
.PKDATA _dv_g, 128;
// The diagnostic block (finding 45): word 0 counts the blocks that reached the hook, words 1-11 are M5, M6, M7, M13, M14, M15,
// I6, I7, B7, L7, MODE1 as the firmware left them at the first voice's entry; word 12 is the last breadcrumb, word 13 counts the
// dispatches to registered types, word 14 the routines that returned (finding 48). core/dsp-link copies the block into the record it sends to the ColdFire each block
// (dl_diag): its words 2 to 15.
.PKDATA _dv_diag, 128;
// The addresses of the two reply records (rings): [0] the one the last block built, [1] the one before it, and [2] the address of the module step word
// `_dk_step` (cleared before a module call). Written by core/dsp-link.
.PKDATA _dv_rec, 12;
// The contract adapter's data: the `dk_voice_in` it hands to the module, the parameter words (8), the blocks since the note-on per track (16), the gates
// per track (16), its scalars (see `_dv_contract`) and the knob positions of the module's parameters in order (ADM_PARAM_SLOT0 .. 7; 255: none).
.PKDATA _dv_in, DK_IN_BYTES;
.PKDATA _dv_params, 32;                    // live effective UI/debug words
.PKDATA _dv_call_params, 32;               // merged callback words (latched declarations over live words)
// Adapter-private snapshots: DT2 track / DN2 physical voice slot, never firmware-owned state.
.PKDATA _dv_trig_params, 512;
.PKDATA _dv_blk, 64;
.PKDATA _dv_gate, 64;
.PKDATA _dv_c, 64;
// Per track: nonzero while the even voice has written R into the odd voice's block this block (contract 1.2); the odd voice consumes it.
.PKDATA _dv_st, 64;
.VAR _dv_slots = ADM_PARAM_SLOT0;
.VAR _dv_slots1 = ADM_PARAM_SLOT1;
.VAR _dv_slots2 = ADM_PARAM_SLOT2;
.VAR _dv_slots3 = ADM_PARAM_SLOT3;
.VAR _dv_slots4 = ADM_PARAM_SLOT4;
.VAR _dv_slots5 = ADM_PARAM_SLOT5;
.VAR _dv_slots6 = ADM_PARAM_SLOT6;
.VAR _dv_slots7 = ADM_PARAM_SLOT7;

#include "filter-hooks.inc"
