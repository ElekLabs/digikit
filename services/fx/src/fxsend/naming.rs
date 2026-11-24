//! Setting keys and module name hashes.

use super::*;
use dkrt_abi::dsp;

/// `prefix` and the module id of effect row `k` into `buf`: the key's length.
pub(super) fn key_of(buf: &mut [u8; KEY_MAX], prefix: &[u8], id: &[u8; 32], idlen: usize) -> usize {
    let mut n = 0;
    while n < prefix.len() {
        buf[n] = prefix[n];
        n += 1;
    }
    let mut i = 0;
    while i < idlen {
        buf[n + i] = id[i];
        i += 1;
    }
    n + idlen
}

pub(super) unsafe fn copy(dst: &mut [u8], src: *const u8, max: usize) {
    let mut i = 0;
    while i < dst.len() {
        dst[i] = 0;
        i += 1;
    }
    i = 0;
    while i < max && i + 1 < dst.len() && *src.add(i) != 0 {
        dst[i] = *src.add(i);
        i += 1;
    }
}

/// FNV-1a 32 of the NUL-terminated module name at `name` (at most 32 bytes): what tells the module of an effect row from another module with the same effect id.
pub unsafe fn name_hash(name: *const u8) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < 32 && *name.add(i) != 0 {
        h = (h ^ *name.add(i) as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// The name hash of the module that owns effect row `k` (kept at `apply`); 0 for a row nobody owns.
pub fn module_hash(k: usize) -> u32 {
    let s = ss();
    if k >= dsp::MAX_FX || s.idlen[k] == 0 {
        return 0;
    }
    unsafe { name_hash(s.id[k].as_ptr()) }
}
