use crate::poll::Poll;
use crate::sent::Sent;

/// The bytes of `core/dsp-link` the queue needs, and the sink of its events.
pub trait Transport {
    /// Start a transfer into window `win` (0: the area, `dst` a byte address; 1 and 2: `dst` a word offset). `buf` is read in place until the poll says Done or Error.
    unsafe fn send(&mut self, win: u32, dst: u32, buf: *const u8, len: u32) -> Sent;
    unsafe fn poll(&mut self) -> Poll;
    /// One event of the log (`what` is an `EV_*`). Called on every event: a transport without a sink returns at once.
    fn event(&mut self, _what: u32, _a: [u32; 4]) {}
}
