//! Memory and string helpers with no state: big-endian words, C strings, volatile words and halfwords. Core and services share them.
use crate::dev_addr::DevAddr;
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

/// Word `k` of an info table (`rows_info`, `dyndsp_info`, `dynfx_info`).
#[inline]
pub unsafe fn iw(i: *const u32, k: usize) -> u32 {
    read_volatile(i.add(k))
}

/// The word at the device address `a` (volatile): the one read path of words.
#[inline(always)]
pub unsafe fn read32(a: DevAddr) -> u32 {
    read_volatile(a.ptr::<u32>())
}

/// Write the word `v` at the device address `a` (volatile): the one write path of words.
#[inline(always)]
pub unsafe fn write32(a: DevAddr, v: u32) {
    write_volatile(a.ptr::<u32>(), v)
}

#[inline]
pub unsafe fn rd(a: u32) -> u32 {
    read32(DevAddr::new(a))
}

#[inline]
pub unsafe fn wr(a: u32, v: u32) {
    write32(DevAddr::new(a), v)
}

/// The big-endian s16 at `a` (read byte by byte: `a` may be odd).
#[inline]
pub unsafe fn rd16(a: u32) -> i32 {
    let a = DevAddr::new(a);
    let hi = read_volatile(a.ptr::<u8>()) as u16;
    let lo = read_volatile(a.add(1).ptr::<u8>()) as u16;
    ((hi << 8 | lo) as i16) as i32
}

#[inline]
pub unsafe fn wr16(a: u32, v: i32) {
    let a = DevAddr::new(a);
    write_volatile(a.ptr::<u8>(), (v >> 8) as u8);
    write_volatile(a.add(1).ptr::<u8>(), v as u8);
}

/// Call the function at the device address `a` with `args` (at most `CALL_ARGS` words) and give its result. The ColdFire C calling convention passes the arguments on the stack
/// and the caller takes them off, so a function that wants fewer arguments reads the first of them. An address of 0 is a word that nobody bound: nothing is called and the answer
/// is 0.
///
/// # Safety
/// `a` is the address of a function of the C calling convention that takes the first words of `args` (the image's code or the firmware's), or 0.
pub unsafe fn call_at(a: DevAddr, args: &[usize]) -> usize {
    if a.is_null() {
        return 0;
    }
    let mut v = [0usize; CALL_ARGS];
    let n = if args.len() < CALL_ARGS {
        args.len()
    } else {
        CALL_ARGS
    };
    let mut i = 0;
    while i < n {
        v[i] = args[i];
        i += 1;
    }
    type F = unsafe extern "C" fn(
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
    ) -> usize;
    // On the device the address is the code address; the host maps it to the host function of its address space.
    #[cfg(target_pointer_width = "32")]
    let f: F = core::mem::transmute(a.get() as usize);
    #[cfg(not(target_pointer_width = "32"))]
    let f: F = core::mem::transmute(a.code_addr());
    f(
        v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11],
    )
}

/// The most arguments `call_at` passes.
pub const CALL_ARGS: usize = 12;

/// Firmware memory and code by address: word reads and writes of the rows and tables of the image that a service fills, and calls of the image's functions. A service
/// takes the core as a `CoreApi`, which has this as a supertrait, so a host test gives a core that logs the writes and the calls (`FakeCore`) instead of dereferencing an address that
/// is not a host pointer.
pub trait FwMem {
    unsafe fn rd(&self, a: u32) -> u32 {
        rd(a)
    }
    unsafe fn wr(&self, a: u32, v: u32) {
        wr(a, v)
    }
    /// The byte at `a`.
    unsafe fn rd8(&self, a: u32) -> u32 {
        read_volatile(DevAddr::new(a).ptr::<u8>()) as u32
    }
    /// The low byte of `v` at `a`.
    unsafe fn wr8(&self, a: u32, v: u32) {
        write_volatile(DevAddr::new(a).ptr::<u8>(), v as u8)
    }
    /// The big-endian s16 at `a`, read byte by byte (`a` may be odd).
    unsafe fn rd16(&self, a: u32) -> i32 {
        rd16(a)
    }
    /// The low halfword of `v` at `a`, written byte by byte (`a` may be odd).
    unsafe fn wr16(&self, a: u32, v: i32) {
        wr16(a, v)
    }
    /// The low halfword of `v` at the even address `a` in one store (a table of the image that other code reads as halfwords).
    unsafe fn wr_half(&self, a: u32, v: u32) {
        write_volatile(DevAddr::new(a).ptr::<u16>(), v as u16)
    }
    /// `call_at`: the function at `a` (0: none, answers 0).
    unsafe fn call(&self, a: u32, args: &[usize]) -> usize {
        call_at(DevAddr::new(a), args)
    }
}

/// Copy `n` bytes from `src` to `dst` (the ranges do not overlap), one byte at a time: no call to the compiler's `memcpy`.
pub unsafe fn copy(dst: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
}
