// core/fx, the send route hook of the Digitakt II (FUN_1c207b, called at 0x1c3099): the stub and the data (docs/findings/62-fx-send-route.md, 67-fx-routes-dn2.md; the description of the route is the header of core/services/fx/dks/dsp/fx-dt2.s).
//
// The image keeps the hook, the stub and the data of the send route (R12-8 of docs/plans/runtime-and-services.md). The dispatch code (the quick scan, the row loop, the send bus mix and the return add)
// is `_fxs_dispatch` of core/services/fx (dks/dsp/fx-dt2.s). Like the master stub (fx.s) the stub reads one slot of core/dsp-link's slot block, DK_SLOT_FX_SEND: 0 restores R0 and continues in
// `_fxs_back` (the displaced instruction, then the master function: stock audio), otherwise it saves M13 and I12 in the words of fx.s, pins M13 = 0 and jumps; the entry restores them.

#include "dk_stub.inc"

.SECTION/PM seg_pmco;
.GLOBAL _fxs_entry;
.GLOBAL _fxs_idle;
.EXTERN _fxs_back;
.EXTERN _dl_slots;
.EXTERN _fx_s_r0;
.EXTERN _fx_sv_m13;
.EXTERN _fx_sv_i12;

_fxs_entry:
    DM(_fx_s_r0) = R0;
    R0 = DM(_dl_slots + DK_SLOT_FX_SEND);
    R0 = PASS R0;
    IF EQ JUMP _fxs_idle;
    DM(_fx_sv_m13) = M13;
    DM(_fx_sv_i12) = I12;
    M13 = 0;
    I12 = R0;
    JUMP (M13, I12);

_fxs_idle:
    R0 = DM(_fx_s_r0);
    JUMP _fxs_back;                       // the displaced instruction, then the master function

.SECTION/DM seg_dmda;
.PKDATA _fxs_bus, 260;                    // the pad word, then the send bus (32 left, 32 right floats)
.PKDATA _fxs_gnew, 64;                    // the 16 new gains
.PKDATA _fxs_prev, 256;                   // the old gains: 16 floats for each of up to 4 rows
// The scalars: 0 the row's index, 8 the track's bus.
.PKDATA _fxs_c, 16;
