// core/fx, the send route (Digitakt II; docs/findings/62-fx-send-route.md): an effect row with route 1 does not process the master. Its input is a send bus: the sum
// over the 16 tracks of (the row's amount for the track) squared times the track's bus. Its wet output times the row's MIX is added to the stock FX return bus,
// which the master function mixes like the return of the stock send effects. The effect's process routine and the dk_fx contract are the same as in the master
// route (64 planar floats in: 32 left, 32 right; wet out).
//
// The source is in parts (fxsend-a.inc, fxsend-tracks.inc, fxsend-b.inc) that fxsend-dn2.s shares, with its own mix of the 16 voice slots (fxsend-slots.inc).
//
// The hook is the first instruction of the master function (FUN_1c207b, called at 0x1c3099; 48 bits, `I7 = MODIFY(I7, -0x14)`, one caller). The sends driver
// (FUN_1c18a6) has finished: the track buses 0x252df8 + 0x100 * t (16 tracks, 32 left then 32 right floats) are intact, the FX return bus 0x254778 is written (by
// the driver at 0x1c1ef2) and not yet read (by the master at 0x1c21ab). The entry is the same kind of hook as fx.s's: the displaced instruction runs in the tail
// (`_fxs_back`), every register is saved by `_fx_enter` of fx.s, which also pins the context, and `_fx_exec` of fx.s runs one row with the output check, the
// fault accounting and the fades. So the guard exists once.
//
// The block counter `_fx_blk` is advanced by the master hook only (once a block, also when the chain has no master row); this hook runs earlier in the block and
// passes the current value on, so a send effect sees the number of the block before.
//
// Row words (fx.s, module.toml): 18 route (0: master chain, 1: send; other values: the row is run by neither), 19 to 22 the amounts of the 16 tracks, one byte
// for each, track t in word 19 + t / 4, byte t % 4 (little endian; track 0 is the least significant byte of word 19). A byte is 0 to 127 (a larger value counts as 127).
// The gain of a track is (byte / 127) squared, as the stock sends square their knob value (FUN_1c18a6: `F2 = F2 * F2`). Each gain is ramped in a block, linear from
// the gain of the last block that ran this row to the new gain: the gain before sample k is old + (k + 1) * step, step = (new - old) / 32, by adding the step
// per sample (the stock sends ramp the same way). The old gains are kept per row in `_fxs_prev`. A track with the old and the new gain 0 costs a few instructions.
//
// Cost (concrete core, tests/patchkit/test_fx_send.py, the half test effect of about 210 instructions): a block with no send row costs the quick scan, 16 instructions
// (no row) to 31 (three free rows), about 8 for each effect row. A send row, steady: 1264 instructions with all amounts 0 (the context save and restore, the output
// check of fx.s, clearing the bus, the 16 gains, the return add: 256), 1414 with one track, each track with a gain adds 150 (4 instructions a sample), 3664 with 16 tracks.
// A track whose old and new gain are 0 costs 10.
//
// Register use: R0 to R15, I0 to I5, I12, I13, M1 to M4, B3 as fx.s (the hook saves them), M6 = 1 and M14 = 1 pinned by `_fx_enter`.
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
.EXTERN _fxs_trackbus;

#include "dk_trace.inc"
#include "fx-rows.inc"
#include "fx-chain.inc"
#include "fxsend-a.inc"
#include "fxsend-tracks.inc"
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
