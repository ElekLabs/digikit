/* <dk/core/trace.h>: log marks that exist only in a traced build. Hand-written.

   DK_TRACE_LOG(msg, value);    dk_log(DK_LOG_TRACE, msg, value): a message (24 bytes kept) and a word in the runtime's log ring.

   `patchkit pack --trace` (also `module:check -- --trace`, `dk:dev -- --trace`) compiles with -DDK_TRACE. Without it the macro
   compiles to nothing: no code, no data, no import, the same object. The arguments are not evaluated then, but they still
   count as used, so a variable that only a DK_TRACE_LOG reads gives no warning. It is a macro, not a function, for that reason. */
#ifndef DK_TRACE_H
#define DK_TRACE_H
#include <dk/core/log.h>

#ifdef DK_TRACE
#define DK_TRACE_LOG(msg, value) dk_log(DK_LOG_TRACE, (msg), (uint32_t)(value))
#else
#define DK_TRACE_LOG(msg, value) ((void)(0 && (value)))
#endif

#endif
