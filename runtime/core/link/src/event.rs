/// The events (`DSP_LINK` of the log): args are `[what, a, b, c, d]`.
/// SUBMIT `[owner, id, kind | steps << 8, len]`, START `[owner, id, 0, 0]`, DONE `[owner, id, 0, 0]`, ERROR `[owner, id, code, position]`,
/// RESEND `[slot index, version, attempt, 0]`, LIVE (a live word taken) `[slot index, version, win, word]`.
/// A live word has the id `0x8000 | slot index` and the owner 0xff.
pub const EV_SUBMIT: u32 = 1;
pub const EV_START: u32 = 2;
pub const EV_DONE: u32 = 3;
pub const EV_ERROR: u32 = 4;
pub const EV_RESEND: u32 = 5;
pub const EV_LIVE: u32 = 6;
