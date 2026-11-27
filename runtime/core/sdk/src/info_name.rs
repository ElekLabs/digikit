/// FNV-1a of `bytes`, the key of `CoreApi::info` (the same hash as `dkneeds::keys::name_hash`).
pub(crate) const fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}
