//! Memory and string helpers with no state: big-endian words, C strings, volatile words and halfwords. Core and services share them.
use core::ptr::{read_volatile, write_volatile};

pub unsafe fn rd32(p: *const u8) -> u32 {
    ((*p as u32) << 24)
        | ((*p.add(1) as u32) << 16)
        | ((*p.add(2) as u32) << 8)
        | (*p.add(3) as u32)
}

pub unsafe fn wr32(p: *mut u8, v: u32) {
    *p = (v >> 24) as u8;
    *p.add(1) = (v >> 16) as u8;
    *p.add(2) = (v >> 8) as u8;
    *p.add(3) = v as u8;
}

/// Compare two NUL-terminated strings.
pub unsafe fn cstr_eq(a: *const u8, b: *const u8) -> bool {
    let mut i = 0;
    loop {
        let x = *a.add(i);
        if x != *b.add(i) {
            return false;
        }
        if x == 0 {
            return true;
        }
        i += 1;
    }
}

/// Length of a NUL-terminated string, at most `max`.
pub unsafe fn cstr_len(a: *const u8, max: usize) -> usize {
    let mut i = 0;
    while i < max && *a.add(i) != 0 {
        i += 1;
    }
    i
}

/// Word `k` of an info table (`dynreg_info`, `dyndsp_info`, `dynfx_info`, `fxpages_info`).
#[inline]
pub unsafe fn iw(i: *const u32, k: usize) -> u32 {
    read_volatile(i.add(k))
}

#[inline]
pub unsafe fn rd(a: u32) -> u32 {
    read_volatile(a as *const u32)
}

#[inline]
pub unsafe fn wr(a: u32, v: u32) {
    write_volatile(a as *mut u32, v)
}

/// The big-endian s16 at `a` (read byte by byte: `a` may be odd).
#[inline]
pub unsafe fn rd16(a: u32) -> i32 {
    let hi = read_volatile(a as *const u8) as u16;
    let lo = read_volatile((a + 1) as *const u8) as u16;
    ((hi << 8 | lo) as i16) as i32
}

#[inline]
pub unsafe fn wr16(a: u32, v: i32) {
    write_volatile(a as *mut u8, (v >> 8) as u8);
    write_volatile((a + 1) as *mut u8, v as u8);
}
