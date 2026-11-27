//! `TransportFns`: the upload transport (`transport.rs`): the reply a command handler builds.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TransportFns {
    /// Send the reply of `cmd`/`seq` with `status` and `extra` body bytes already written at `reply_body() + 1`.
    pub reply: unsafe extern "C" fn(cmd: u32, seq: u32, status: i32, extra: usize) -> i32,
    /// The reply body buffer (`REPLY_BODY_MAX` bytes): byte 0 is the status, the body starts at byte 1.
    pub reply_body: unsafe extern "C" fn() -> *mut u8,
}
