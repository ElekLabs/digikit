//! The firmware identity of the flashed image.
use crate::platform::Platform;

/// The `fw_id` entry of the image's info table: the first 8 bytes of the SHA-256 of the stock firmware (two words, kept big endian); `None` for an image that has none.
pub unsafe fn image_fw_id<P: Platform>(p: &P) -> Option<[u8; 8]> {
    let at = p.info(dkneeds::keys::name_hash("fw_id"));
    if at.is_null() {
        return None;
    }
    let mut id = [0u8; 8];
    id[..4].copy_from_slice(&core::ptr::read_volatile(at).to_be_bytes());
    id[4..].copy_from_slice(&core::ptr::read_volatile(at.add(1)).to_be_bytes());
    Some(id)
}
