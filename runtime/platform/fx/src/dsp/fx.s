// core/fx: the effects adapter of the SHARC engine, the hook stub and the data (docs/findings/55, <dk/fx/fx.h>; R12-8 of docs/plans/runtime-and-services.md).
//
// The image keeps the hook, the data the dispatch code and the effects share, and a stub. The dispatch (the quick scan, the row loop, `_fx_exec`, the output guard, the fades) is
// fx-chain.inc of the effect-registry owner, which carries (dks/dsp/fx-<device>.s). The send route has its own stub and data (fxsend.s, fxsend-dn2.s).
//
// The hook is the first instruction of a function that the block orchestrator calls once per block after the master mix and before the USB pack: Digitakt II FUN_1c80f2 (called at
// 0x1c311f, after the pack's earlier siblings and before the pack at 0x1c3160), Digitone II FUN_1c244d (H1 of finding 53, which also holds the pack). The site is chosen for the
// emulator's AOT core, not for the firmware: a patched code range bars the generated regions of that range (finding 55). patchkit overwrites the 48-bit first instruction
// (`I7 = MODIFY(I7, imm)`) with `JUMP _fx_entry`; the displaced instruction runs in the hook's tail (`_fx_back`), which jumps to the second instruction of the stock function. At the
// entry nothing of the stock frame exists yet, so the stub saves what it uses and the dispatch code saves the rest.
//
// The stub reads one slot of core/dsp-link's slot block (`_dl_slots`; sdk/dsp/contract/dk_stub.inc):
//   slot 0: no dispatch code is installed (the effect-registry owner is not loaded, or failed): R0 is restored and the stub continues in `_fx_back`: the audio is the stock audio.
//   else:   the pc of the dispatch entry. The stub saves M13 and I12 in `_fx_sv_m13` and `_fx_sv_i12`, pins M13 = 0 and jumps with `JUMP (M13, I12)`. The entry restores them.
// R0 is saved in `_fx_s_r0` on both paths; `_fx_idle` restores it. The dispatch code ends in `_fx_idle` as well, after it has restored every other register.

#include "dk_stub.inc"

.SECTION/PM seg_pmco;
.GLOBAL _fx_entry;
.GLOBAL _fx_idle;
.EXTERN _fx_back;
.EXTERN _dl_slots;

_fx_entry:
    DM(_fx_s_r0) = R0;
    R0 = DM(_dl_slots + DK_SLOT_FX_ENTRY);
    R0 = PASS R0;
    IF EQ JUMP _fx_idle;
    DM(_fx_sv_m13) = M13;
    DM(_fx_sv_i12) = I12;
    M13 = 0;
    I12 = R0;
    JUMP (M13, I12);

_fx_idle:
    R0 = DM(_fx_s_r0);
    JUMP _fx_back;                        // the displaced instruction, then the stock function

.SECTION/DM seg_dmda;
.PKDATA _fx_save, 128;                    // R1-R15, I0-I3, I5, I12, I13, M1-M4, B3
.PKDATA _fx_s_r0, 4;
.PKDATA _fx_sv_m13, 4;                      // the registers the stubs pinned (dk_stub.inc; the send stub shares them: the two hooks never overlap)
.PKDATA _fx_sv_i12, 4;
.PKDATA _fx_s_i4, 4;
.PKDATA _fx_s_l4, 4;
.PKDATA _fx_ctx, 48;                      // M5, M6, M7, M13, M14, M15, L0-L3, L5, MODE1 of the context
.PKDATA _fx_blk, 4;                       // blocks with an effect row in the chain (counted by the master hook; the send pass passes it on)
.PKDATA _fx_in, 12;                       // the dk_fx_in
.PKDATA _fx_scratch, 256;                 // the effect's output block
// The adapter's scalars: 0 row, 4 mode, 8 enabled, 12 result.
.PKDATA _fx_c, 16;
// Diagnostics (copied into the fault table by core/dsp-link): 0 `last` (see _fx_mark), 1 faults of all effects, 2 row (byte offset + 1) of the last fault, 3 row
// (byte offset + 1) switched off last, 4 module calls of the master route, 5 module calls of the send route.
.PKDATA _fx_diag, 32;
// The addresses of the two reply records (rings): [0] the one the last block built, [1] the one before it, and [2] the address of the module step word
// `_dk_step` (the mark clears it). Written by core/dsp-link.
.PKDATA _fx_rec, 12;

