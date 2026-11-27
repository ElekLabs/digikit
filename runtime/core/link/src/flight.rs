pub(crate) const F_NONE: u8 = 0;
pub(crate) const F_LIVE: u8 = 1;
pub(crate) const F_CHUNK: u8 = 2;
pub(crate) const F_STEP: u8 = 3;
pub(crate) const F_UNDO: u8 = 4;

/// The transfer in flight.
#[derive(Clone, Copy)]
pub(crate) struct Flight {
    pub(crate) kind: u8,
    /// The live slot or the request index.
    pub(crate) idx: u8,
    pub(crate) win: u8,
    pub(crate) ver: u16,
    /// The word of a live write.
    pub(crate) word: u32,
    /// The value of a live write; the bytes of a chunk.
    pub(crate) value: u32,
}
pub(crate) const FLIGHT0: Flight = Flight {
    kind: F_NONE,
    idx: 0,
    win: 0,
    ver: 0,
    word: 0,
    value: 0,
};
