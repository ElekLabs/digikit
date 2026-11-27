//! The upload transport: the reply a command handler builds.
use super::table;

/// Send the reply for `cmd`/`seq`: status, then the `extra` bytes already in the reply body at `rbody() + 1`.
#[inline(always)]
pub(crate) unsafe fn reply(cmd: u8, seq: u32, status: i32, extra: usize) -> i32 {
    (table().transport.reply)(cmd as u32, seq, status, extra)
}

/// The reply body buffer: byte 0 is the status, the body starts at byte 1.
#[inline(always)]
pub(crate) unsafe fn rbody() -> *mut u8 {
    (table().transport.reply_body)()
}
