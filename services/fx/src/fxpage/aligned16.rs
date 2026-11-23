//! A 16-byte buffer for the firmware string that `make_rep` writes in words.

/// Four-byte alignment for a buffer that `make_rep` writes in words.
#[repr(C, align(4))]
pub struct Aligned16(pub [u8; 16]);
