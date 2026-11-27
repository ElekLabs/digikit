//! The RAM image.
use super::*;

/// A RAM image: the bytes of a file, 16-byte aligned (`dk_files_write` needs it).
#[repr(C, align(16))]
pub struct Image(pub [u8; AREA]);

impl Image {
    pub const fn new() -> Image {
        Image([0; AREA])
    }
}

impl Default for Image {
    fn default() -> Self {
        Self::new()
    }
}
