//! C strings at the SDK boundary: a NUL-terminated copy of a store path, and a view of a NUL-terminated name.

/// A NUL-terminated copy of a store path (at most 63 bytes, which `Path` never exceeds).
pub(super) fn cpath(p: &[u8], out: &mut [u8; 64]) -> *const u8 {
    let n = p.len().min(63);
    let mut i = 0;
    while i < n {
        out[i] = p[i];
        i += 1;
    }
    out[n] = 0;
    out.as_ptr()
}

/// The bytes of a NUL-terminated string of at most `max` bytes.
pub(super) unsafe fn cstr<'a>(p: *const u8, max: usize) -> &'a [u8] {
    let mut n = 0;
    while n < max && *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n)
}
