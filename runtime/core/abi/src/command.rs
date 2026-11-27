//! `Cmd`: one upload command of the resident core or the system package, and its flags. The resident core keeps its own table of them (`transport.rs`) and the package
//! keeps its own (`commands.rs`), so both name this type.

/// What a command handler gets: the request number, the transport buffer, the body and its length; it returns the status it answered with.
pub type Handler = unsafe fn(seq: u32, buf: *mut u8, body: *const u8, blen: usize) -> i32;

/// The reply of this command is not logged as an UPLOAD event (it is read-only and frequent).
pub const NOLOG: u8 = 1;
/// The body is read in place (`buf + BODY_AT`, up to a chunk), not copied into the small request buffer.
pub const BIG: u8 = 2;
/// The command reads or writes the table or the card through the store: it holds the store lock.
pub const STORE: u8 = 4;

/// Bytes of the reply body a handler may build (the wire bytes of a reply are at most 1 KiB: the unpacked body at most this).
pub const REPLY_BODY_MAX: usize = 880;

pub struct Cmd {
    pub cmd: u8,
    pub flags: u8,
    pub h: Handler,
}
