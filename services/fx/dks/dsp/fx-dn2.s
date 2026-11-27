// core/fx, the send route on the Digitone II (docs/findings/67-fx-routes-dn2.md): the send route of fxsend.s with the slot mix instead of the track mix. An effect row with
// route 1 does not process the master. Its input is a send bus: the sum over the 16 voice slots of (the row's amount for the slot's track) squared times the slot's bus. Its wet
// output times the row's MIX is added to the stock FX return bus, which the master function mixes like the return of the stock send effects.
//
// The hook is the first instruction of the master function (FUN_1c1f1e, called at 0x1c305d; 48 bits, `I7 = MODIFY(I7, -16)`, one caller). The sends driver (FUN_1c1738) has
// finished: the 16 slot buses 0x257ee8 + 0x100 * v (32 left then 32 right floats; the same buses the driver reads) are intact, the FX return bus 0x25b868 is written (by the
// driver at 0x1c1d96) and not yet read (by the master at 0x1c20fa). The master function itself sums the slots into the track buses (0x258f68 + 0x100 * t), after clearing them;
// the send hook sums the slot buses on its own, with the map the master uses (0x257898). The master mixes the slot buses and the FX return with the same factor (14.126), so the
// return of a unity effect on amount 127 is the track's own sound. Everything else is fxsend.s.
//
// Register use, the row words, the amounts, the gain law and the cost model: fxsend.s (the slot mix costs the 16 map entries more: about 100 instructions).
//
// The dispatch object of the fx service for this device (R12-8 of docs/plans/runtime-and-services.md): the master chain (fx-chain.inc, entry `_fx_dispatch`, slot fx_entry) and the send
// route (entry `_fxs_dispatch`, slot fx_send). The image keeps the stubs, `_fx_idle` and `_fxs_idle` (R0, the displaced instruction and the stock function) and all data (core/fx's fx.s
// and the send data file); everything it names reaches this object through the DSP imports of core/dyndsp. The device's header above is that of the send hook before the split.

.SECTION/PM seg_pmco;
.GLOBAL _fx_dispatch;
.GLOBAL _fxs_dispatch;
.EXTERN _fx_table;
.EXTERN _fx_master;
.EXTERN _fx_sv_m13;
.EXTERN _fx_sv_i12;
.EXTERN _fx_idle;
.EXTERN _fx_s_i4;
.EXTERN _fx_s_l4;
.EXTERN _fx_save;
.EXTERN _fx_ctx;
.EXTERN _fx_blk;
.EXTERN _fx_in;
.EXTERN _fx_scratch;
.EXTERN _fx_c;
.EXTERN _fx_diag;
.EXTERN _fx_rec;
.EXTERN _fxs_idle;
.EXTERN _fxs_return;
.EXTERN _fxs_bus;
.EXTERN _fxs_gnew;
.EXTERN _fxs_prev;
.EXTERN _fxs_c;
.EXTERN _fxs_slotbus;
.EXTERN _fxs_slotmap;
.EXTERN _fxs_fadetab;
.EXTERN _fxs_trk;
.EXTERN _fxs_flg;
.EXTERN _fxs_fbuf;

#include "dk_trace.inc"
#include "fx-rows.inc"
#include "fx-chain.inc"
#include "fxsend-a.inc"
#include "fxsend-slots.inc"
#include "fxsend-b.inc"

// The stats rows of the spans (sdk/dsp/contract/dk_span.inc): rows 8 to 15 are the effect rows (24 words each in the effect table; the last takes the rest).
#define DK_SPAN_PTR_WORD _fx_diag + 28
#define DK_SPAN_ROW_CODE \
    R2 = I4; \
    R0 = _fx_table; \
    R2 = R2 - R0; \
    R0 = LSHIFT R2 BY 3; \
    R0 = R0 + R2; \
    R2 = LSHIFT R2 BY 1; \
    R2 = R0 + R2; \
    R2 = LSHIFT R2 BY -10; \
    R0 = 8; \
    R2 = R2 + R0; \
    R0 = 15; \
    R2 = MIN(R2, R0);
#include "dk_span.inc"
