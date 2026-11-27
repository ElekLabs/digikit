//! The call function of each method and the query table (`dk_workbench_queries`, <dk/query.h>).
//!
//! The call function decodes the request, calls the handler the author writes, and encodes the reply into the caller's buffer, bounded
//! by `*out_len`. A request that does not decode is `DK_E_FORMAT`; a reply that does not fit (or breaks its own bounds) is
//! `DK_E_TOO_BIG`; a negative handler result is passed on as it is.

use super::CCodec;
use std::fmt::Write as _;

impl CCodec<'_> {
    pub(super) fn service(&mut self) {
        let Some(service) = self.schema.service.clone() else {
            return;
        };
        let prefix = self.prefix.clone();
        let n = service.methods.len();
        writeln!(
            self.head,
            "\n/* Service {0}: the handlers the module implements, one per method. A handler fills the reply (zeroed before the call) and returns\n * 0, or a negative DK_E_* code that the caller sees as a refusal. The strings of a request are not checked for UTF-8. */",
            service.name
        )
        .unwrap();
        for m in &service.methods {
            writeln!(
                self.head,
                "int32_t {prefix}_{}(dk_module_ctx *ctx, const {} *request, {} *reply);",
                m.handler,
                self.ty(m.request),
                self.ty(m.reply)
            )
            .unwrap();
        }
        writeln!(
            self.head,
            "\n/* The query table for dk_workbench_register_queries: method numbers are positions in the service, from 1. */\n#define DK_WORKBENCH_QUERY_COUNT {n}u\nextern const dk_workbench_query dk_workbench_queries[DK_WORKBENCH_QUERY_COUNT];"
        )
        .unwrap();
        for m in &service.methods {
            let (req, rep) = (self.ty(m.request), self.ty(m.reply));
            let h = &m.handler;
            writeln!(
                self.out,
                "\nstatic int32_t {prefix}_call_{h}(dk_module_ctx *ctx, const uint8_t *in, uint32_t in_len, uint8_t *out, uint32_t *out_len)\n{{\n    {req} request;\n    {rep} reply;\n    dk_wire_writer w;\n    int32_t rc;\n    if ({req}_decode(in, in_len, &request) < 0)\n        return DK_E_FORMAT;\n    dk_wire_zero(&reply, sizeof reply);\n    rc = {prefix}_{h}(ctx, &request, &reply);\n    if (rc < 0)\n        return rc;\n    dk_wire_writer_init(&w, out, *out_len);\n    {rep}_encode(&w, &reply);\n    if (w.err)\n        return DK_E_TOO_BIG;\n    *out_len = (uint32_t)(w.p - out);\n    return 0;\n}}"
            )
            .unwrap();
        }
        writeln!(
            self.out,
            "\nconst dk_workbench_query dk_workbench_queries[DK_WORKBENCH_QUERY_COUNT] = {{"
        )
        .unwrap();
        for (i, m) in service.methods.iter().enumerate() {
            writeln!(
                self.out,
                "    {{{prefix}_call_{}, {}, {{0}}}},",
                m.handler,
                i + 1
            )
            .unwrap();
        }
        writeln!(self.out, "}};").unwrap();
    }
}
