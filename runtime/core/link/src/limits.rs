/// Requests the queue holds at once (the loader, the track writer, the core's clears and one spare).
pub const REQ_MAX: usize = 3;
/// Live words: 4 effect rows of 8 page words (7 knobs and MIX) and 4 send amounts.
pub const SLOT_MAX: usize = 48;
/// The bytes of one transfer of a bulk. A multiple of 4.
pub const CHUNK: u32 = 4096;
/// Pump passes a live word waits after a failed write before it is sent again.
pub(crate) const HOLD: u8 = 4;
/// Polls a blocking wait spins before it gives up (the same bound as the loader's one-word write had).
pub(crate) const SPIN: u32 = 4_000_000;
