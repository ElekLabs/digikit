/* <dk/query.h>: the query table a module hands to the runtime (dk_workbench_register_queries). patchkit generates the table, the call
 * functions and the handler prototypes from the module's workbench/<name>.proto (docs/MODULE-AUTHORING.md "Queries"); a module does not
 * write this struct itself. `method` is the method's 1-based position in the schema's `service`. `call` decodes the request bytes,
 * runs the module's handler, and writes the encoded reply to `out`, whose size is *out_len on entry and the reply size on return.
 * Returns 0, DK_E_FORMAT (the request does not decode), DK_E_TOO_BIG (the reply does not fit) or the handler's negative DK_E_* code.
 * Strings are not checked for UTF-8 on the device: the page checks what it shows. */
#ifndef DK_QUERY_H
#define DK_QUERY_H
#include <stdint.h>
#include <dk/gen/imports.h>

typedef struct {
    uint8_t method;
    int32_t (*call)(dk_module_ctx *, const uint8_t *in, uint32_t in_len, uint8_t *out, uint32_t *out_len);
} dk_workbench_query;
#endif
