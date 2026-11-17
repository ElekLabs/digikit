// dk_stub.inc: the contract between the image's hook stubs and the dispatch code of a service (docs/plans/runtime-and-services.md, R12-8). Hand-written.
//
// A hook site of the image keeps a stub. The stub reads one slot, a word of core/dsp-link's slot block (`_dl_slots`, window 0 words 0 to 15 of the link). A slot of 0 means that no
// service owns the site: the stub runs the displaced instruction and the stock path. Any other value is the pc of the service's dispatch entry. The stub then saves the
// registers it pins in the image words named below, pins M13 = 0 and jumps with `JUMP (M13, I12)`. The entry restores the saved registers first, so the dispatch code sees
// the registers as the firmware left them at the hook.
//
// A slot index is a byte offset into `_dl_slots`. The text of this file is part of the service ABI hash (`SVC_ABI_FILES` of patchkit sdkgen): a service built against another stub contract is refused (E_SVC_ABI).

#define DK_SLOTS 16
#define DK_SLOT_BYTES 64
#define DK_SLOT_VOICE_RENDER 0
#define DK_SLOT_VOICE_UNPACK 4
#define DK_SLOT_FX_ENTRY 8
#define DK_SLOT_FX_SEND 12

// Image words (core/dsp-voice, DT2) in which the stubs keep the registers they pinned. The entry of the service restores them: `_dv_sv_m13` (render stub: M13), `_dv_sv_m13u` and `_dv_sv_i12u` (unpack stub: M13, I12).
