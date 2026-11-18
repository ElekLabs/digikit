// core/fx, the send route hook of the Digitakt II (FUN_1c207b, called at 0x1c3099): the stub and the data (docs/findings/62-fx-send-route.md, 67-fx-routes-dn2.md; the description of the route is the header of core/services/fx/dks/dsp/fx-dt2.s).
//
// The image keeps the hook, the stub and the data of the send route (R12-8 of docs/plans/runtime-and-services.md). The dispatch code (the quick scan, the row loop, the send bus mix and the return add)
// is `_fxs_dispatch` of core/services/fx (dks/dsp/fx-dt2.s). Like the master stub (fx.s) the stub reads one slot of core/dsp-link's slot block, DK_SLOT_FX_SEND: 0 restores R0 and continues in
// `_fxs_back` (the displaced instruction, then the master function: stock audio), otherwise it saves M13 and I12 in the words of fx.s, pins M13 = 0 and jumps; the entry restores them.

#include "fxsend-stub.inc"

// The scalars: 0 the row's index, 8 the track's bus.
.PKDATA _fxs_c, 16;
