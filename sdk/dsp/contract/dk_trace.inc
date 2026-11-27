// dk_trace.inc: step marks for the fault record (docs: sdk/README.md, "Tracing"). Hand-written.
//
// DK_STEP(N, REG);   store the step number N (a small constant) in the word `_dk_step`, using the scratch register REG.
//
// The core clears `_dk_step` before each call into a module; the block tail copies it into the block's reply record. The fault record prints the step of the
// last block that ended if the module faults or hangs ("FX SEND STEP 3"); a step stored in the call that stops the DSP is lost with the core. SHARC+ cannot store an immediate to memory, so the macro needs a register that is free at that point.
// `patchkit pack --trace` defines DK_TRACE. Without it DK_STEP expands to nothing: no instruction, no import, the same object.
// The caller writes the trailing ";". Include this file once per module, before the routine files.
#ifndef DK_TRACE_INC
#define DK_TRACE_INC

#ifdef DK_TRACE
.EXTERN _dk_step;
#define DK_STEP(N, REG) \
    REG = N; \
    DM(_dk_step) = REG
#else
#define DK_STEP(N, REG) ;
#endif

// ---- spans (docs/plans/sdk.md section 9, docs/plans/profiling.md 13.1) ----
//
// What a span is: the call of the module's own routine, nothing else. In the machines service one span covers each call of the module's note_on and each call of its render (the contract
// adapter's work around them, the guard and idle or disabled voices are outside), in the fx service the call of the effect's process routine. A voice that is idle or whose type is disabled
// takes no span, so a row's calls are the module's real calls on both devices. A row of the old form (a routine with no note_on) is not measured.
//
// DK_SPAN_BEGIN(SKIP, PTR, RP, IX);   reads the cycle counter into the window's span word, when the stats window is on.
// DK_SPAN_END(SKIP, PTR, RP, IX);     reads it again and adds the span to the row `DK_SPAN_ROW_CODE` names.
//
// Only the dispatch code of the services (machines, fx) uses these, around the call of a module's entry (never a content module: `mise run check:layers`, rule
// spans). They are always assembled in. PTR is a DM word that holds the window's byte address while stats are on and 0 while they are off: core/dsp-link writes
// it (`_dv_diag` word 31 for the machines, `_fx_diag` word 7 for the effects), so that the service imports nothing new. With stats off each macro is a load, a
// compare and a jump (three instructions) and nothing else runs: the counter is not read (reading EMUCLK takes the emulator out of the fast tiers,
// `UNSAFE_READ_NAMES` of tools/sharc_dev/sharc_rsgen.py). RP must be R3 and IX I1, both free at the site; they are lost when stats are on. The routines the
// macros call (dk_span.inc, included once into the object) save and restore R0 to R2 in the window's private words.
//
// The window (the same numbers as <dk/core/trace.h> for the ColdFire side), in words: a header of DK_STATS_HEAD_WORDS (blocks, largest stock block delta,
// overruns, the start of the running span), then DK_STATS_ROWS rows of DK_STATS_ROW_WORDS in the order calls, cycles low, cycles high, largest call, largest
// block, the sum of the running block; then DK_STATS_PRIV_WORDS private words. Sums are 64-bit; core/dsp-link folds each block.
#define DK_STATS_VERSION 2
#define DK_STATS_HEAD_WORDS 4
#define DK_STATS_ROW_WORDS 6
#define DK_STATS_ROWS 16
#define DK_STATS_WORDS 100
#define DK_STATS_W_BLOCKS 0
#define DK_STATS_W_STOCK 1
#define DK_STATS_W_OVERRUNS 2
#define DK_STATS_W_T0 3
#define DK_STATS_ROW_CALLS 0
#define DK_STATS_ROW_CYCLES 1
#define DK_STATS_ROW_CYCLES_HI 2
#define DK_STATS_ROW_LARGEST 3
#define DK_STATS_ROW_BLOCK 4
#define DK_STATS_ROW_SUM 5
#define DK_STATS_PRIV 100

#define DK_SPAN_BEGIN(SKIP, PTR, RP, IX) \
    RP = DM(PTR); \
    RP = PASS RP; \
    IF EQ JUMP SKIP; \
    IX = RP; \
    CALL _dk_span_begin; \
    SKIP:

#define DK_SPAN_END(SKIP, PTR, RP, IX) \
    RP = DM(PTR); \
    RP = PASS RP; \
    IF EQ JUMP SKIP; \
    IX = RP; \
    CALL _dk_span_end; \
    SKIP:

#endif
