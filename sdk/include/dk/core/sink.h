/* <dk/core/sink.h>: the runtime's sink channels (dk_sink_set). Hand-written.

   dk_sink_set(channel, sink) sets (with 0: clears) the sink of one channel for the calling module. A channel has at most one owner: a second module that
   sets it gets DK_E_BUSY. The runtime clears a channel after its owner's fini (a module may also clear it itself). A sink runs in the task of the producer
   with interrupts masked: short, no waiting, no logging, no imports.

   DK_SINK_LOG: `void sink(const dk_log_ev *e)`, every event the runtime logs, before any filter.
   DK_SINK_SPAN: `void sink(const dk_span *s)`, one record per measured span. Without a sink a producer pays one flag test.

   dk_span_sink_set(emit, clock) takes the span channel like dk_sink_set(DK_SINK_SPAN, emit) and also stores the clock, `uint32_t clock(void)`, a counter
   that wraps. The runtime measures a span only while both are set; it reads the clock before and after the measured code and passes the difference as
   `cycles`. On runtime 0.11.3 and later it also passes the first reading as `start_tick`. Check that `dk_module_ctx_runtime_version(ctx)` is at least
   `DK_SPAN_START_TICK_VERSION` before reading that field. (0, 0) clears both. DK_E_BUSY when another module owns the channel. */
#ifndef DK_SINK_H
#define DK_SINK_H
#include <dk/core/module.h>

#define DK_SINK_LOG 0u
#define DK_SINK_SPAN 1u
#define DK_SPAN_START_TICK_VERSION 0x000b03u

/* One span: the producer's owner id, elapsed cycles and the raw clock reading at the start. */
typedef struct dk_span {
    uint32_t owner;
    uint32_t cycles;
    uint32_t start_tick;
} dk_span;
#endif
