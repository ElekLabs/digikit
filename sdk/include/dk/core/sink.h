/* <dk/core/sink.h>: the runtime's sink channels (dk_sink_set). Hand-written.

   dk_sink_set(channel, sink) sets (with 0: clears) the sink of one channel for the calling module. A channel has at most one owner: a second module that
   sets it gets DK_E_BUSY. The runtime clears a channel after its owner's fini (a module may also clear it itself). A sink runs in the task of the producer
   with interrupts masked: short, no waiting, no logging, no imports.

   DK_SINK_LOG: `void sink(const dk_log_ev *e)`, every event the runtime logs, before any filter.
   DK_SINK_SPAN: `void sink(const dk_span *s)`, one record per measured span. Without a sink a producer pays one flag test. */
#ifndef DK_SINK_H
#define DK_SINK_H
#include <dk/core/module.h>

#define DK_SINK_LOG 0u
#define DK_SINK_SPAN 1u

/* One record of the span channel: the owner id the producer gave and the cycles it measured. */
typedef struct dk_span {
    uint32_t owner;
    uint32_t cycles;
} dk_span;
#endif
