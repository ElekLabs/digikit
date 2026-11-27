//! The limits a query schema must meet, checked when the code is generated: a request and a reply travel in one MODULE_UI command each
//! (docs/plans/workbench-module-ui.md "Queries"), and the call function holds both structs on the UI task's stack.

use super::c_codec::{max_sizes, struct_sizes};
use super::schema::Schema;
use crate::error::*;

/// The command body is at most 160 bytes; the call verb's envelope takes 11 (verb, record, instance, request id, method).
pub const REQUEST_MAX: u32 = 149;
/// The reply budget of the call: the host command reply body (879 bytes) less the poll envelope.
pub const REPLY_MAX: u32 = 855;
/// The request struct and the reply struct together, as the call function keeps them on the stack.
pub const STACK_MAX: u32 = 1024;

/// Refuses a method whose request or reply can be larger than the transport allows, or whose structs are over the stack budget.
pub(super) fn check(schema: &Schema) -> Result<()> {
    let Some(service) = &schema.service else {
        return Ok(());
    };
    let encoded = max_sizes(schema);
    let structs = struct_sizes(schema);
    for m in &service.methods {
        let who = format!("{}.{}", service.name, m.name);
        let (request, reply) = (encoded[m.request], encoded[m.reply]);
        if request > REQUEST_MAX {
            return Err(err!(
                "{who}: the request can be {request} bytes, at most {REQUEST_MAX} fit in a command; lower a max_len or max_count"
            ));
        }
        if reply > REPLY_MAX {
            return Err(err!(
                "{who}: the reply can be {reply} bytes, at most {REPLY_MAX} fit in an answer; lower a max_len or max_count"
            ));
        }
        let stack = structs[m.request].saturating_add(structs[m.reply]);
        if stack > STACK_MAX {
            return Err(err!(
                "{who}: the request and reply structs take {stack} bytes of stack, at most {STACK_MAX} are allowed; lower a max_len or max_count"
            ));
        }
    }
    Ok(())
}
