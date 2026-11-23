use crate::provider::Query;
use core::ffi::c_void;

/// The newest version of the query part the core knows.
pub const QUERY_NEWEST: u16 = 1;

/// The query part, version 1 (tag `PART_QUERY`): the shape of the resident query entry `ResidentQueryV1` with the instance first. The result is the length of the
/// reply or a negative status.
#[repr(C)]
pub struct QueryV1 {
    pub query: unsafe extern "C" fn(*mut c_void, u32, *const u8, u32, *mut u8, u32) -> i32,
}

// A table is immutable static data.
unsafe impl Sync for QueryV1 {}

impl QueryV1 {
    /// The table of owner object `P`.
    pub const fn of<P: Query>() -> QueryV1 {
        QueryV1 { query: query::<P> }
    }
}

unsafe extern "C" fn query<P: Query>(
    inst: *mut c_void,
    op: u32,
    req: *const u8,
    req_len: u32,
    reply: *mut u8,
    reply_len: u32,
) -> i32 {
    let req = if req_len == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(req, req_len as usize)
    };
    let reply = if reply_len == 0 {
        &mut [][..]
    } else {
        core::slice::from_raw_parts_mut(reply, reply_len as usize)
    };
    match (*(inst as *mut P)).query(op, req, reply) {
        Ok(len) => len as i32,
        Err(status) => status,
    }
}
