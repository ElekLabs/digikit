/* <dk/core/trace.h>: log marks that exist only in a traced build. Hand-written.

   DK_TRACE_LOG(msg, value);    dk_log(DK_LOG_TRACE, msg, value): a message (24 bytes kept) and a word in the runtime's log ring.

   `patchkit pack --trace` (also `module:check -- --trace`, `dk:dev -- --trace`) compiles with -DDK_TRACE. Without it the macro
   compiles to nothing: no code, no data, no import, the same object. The arguments are not evaluated then, but they still
   count as used, so a variable that only a DK_TRACE_LOG reads gives no warning. It is a macro, not a function, for that reason.

   DK_SPAN_BEGIN(owner); ...; DK_SPAN_END(owner);    measures the cycles of the code between them and hands (owner, cycles) to the span sink (<dk/core/sink.h>).
   Only the dispatcher of a kind (a service) uses it, around its per-row call; a content module or an app may not (`mise run check:layers`, rule spans).
   It is always compiled in. With no sink set, each of the two macros costs one load and one branch on the sink's pointer, and `owner` is not evaluated
   (the cycle counter is not read either). BEGIN declares the local `dk_span_t0_`, so one pair fits a block: put a second pair in a block of its own, and do
   not nest spans of one block. The cycle counter and the emit are the runtime's: `dk_span_sink` (the span channel's sink pointer, a data import), `dk_span_clock`
   and `dk_span_emit` are not imports yet; nothing uses them. The DK_SPAN macros are for service dispatchers (for example `machines` around each voice
   render, `fx` around each process call). Content modules never contain DK_SPAN. The spans of P8a come from the runtime's own call sites (module ticks,
   service phases, polls) through `dk_span_sink_set(emit, clock)` (<dk/core/sink.h>), not from these macros.
   The DSP side has the same macros in dk_trace.inc and the stats window of the layout below. */
#ifndef DK_TRACE_H
#define DK_TRACE_H
#include <dk/core/log.h>

/* The runtime's side of DK_SPAN. */
extern volatile uint32_t dk_span_sink;
uint32_t dk_span_clock(void);
void dk_span_emit(uint32_t owner, uint32_t cycles);

#define DK_SPAN_BEGIN(owner)                                                                       \
    uint32_t dk_span_t0_ = 0;                                                                      \
    if (dk_span_sink)                                                                              \
        dk_span_t0_ = dk_span_clock();                                                             \
    (void)sizeof(owner)
#define DK_SPAN_END(owner)                                                                         \
    do {                                                                                           \
        if (dk_span_sink)                                                                          \
            dk_span_emit((owner), dk_span_clock() - dk_span_t0_);                                  \
    } while (0)

/* The stats window of the DSP side (P8d, docs/plans/profiling.md 13.1; the layout of dk_trace.inc, the same numbers): 100 words in DSP data memory. The
   services' dispatch code adds the spans of each module call, core/dsp-link folds each block and streams the window through the acknowledgement slot, one page
   (two words) per block; the runtime's mirror assembles the pages and `dk_dsp_stats_read` hands the newest complete sweep to a module (the debug module's
   PROFILE_DSP). A module controls it with `dk_dsp_stats_ctl`, and only the owner of the span channel may.
   header, 4 words: 0 blocks folded, 1 the largest stock block delta (the engine's own measure of a block, in cycles), 2 overruns (blocks whose stock delta
   reached the block period), 3 scratch (the start of the span running).
   then DK_STATS_ROWS rows of DK_STATS_ROW_WORDS words: 0 calls, 1 cycles low, 2 cycles high (the sum is 64 bits), 3 the largest call, 4 the largest block
   (the largest sum of one block's calls), 5 scratch (the sum of the running block). Rows 0 to 7 are the machines (the voices registry order; the last row takes
   the rest), rows 8 to 15 the effects. A counter wraps at 32 bits: the reader takes differences. A cycle is a count of the core's cycle counter (EMUCLK): on the
   emulator an instruction, not a cycle. */
#define DK_STATS_VERSION 2u
#define DK_STATS_HEAD_WORDS 4u
#define DK_STATS_ROW_WORDS 6u
#define DK_STATS_ROWS 16u
#define DK_STATS_WORDS 100u
#define DK_STATS_PAGES 50u

#ifdef DK_TRACE
#define DK_TRACE_LOG(msg, value) dk_log(DK_LOG_TRACE, (msg), (uint32_t)(value))
#else
#define DK_TRACE_LOG(msg, value) ((void)(0 && (value)))
#endif

#endif
