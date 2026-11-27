//! `lfo` service, the owner's table of shapes: who keeps each wave number, its name, whether it is unipolar and its 32 samples, and the pure functions over it: the lookups of
//! the UI task (`shape`, `name_of`, `highest`, ...) and the interpolation the graph uses. The wave the LFO plays is not here: the samples reach the image's table of added
//! waves through [`crate::wave_table::WaveTable`]. This table is a named static (the UI hooks of the file reach it by symbol), written and read in the UI task only.
use crate::records::{MAX_SHAPES, NAME_LEN, RANGE, SAMPLES};
use core::ptr::{addr_of, addr_of_mut};

/// `repr(C)` with `owner` first: a loop over the table that compares the address of the `owner` of the element after the last one with a running pointer makes a relocated
/// word that points at the end of `SHAPES`, not past it (`shift_bss_words` of patchkit accepts a word up to the end of `.bss` only).
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Shape {
    /// module record index + 1; 0: free
    pub(crate) owner: u8,
    /// the shape is unipolar: no sample is negative
    pub(crate) unipolar: bool,
    pub(crate) name: [u8; NAME_LEN],
    /// the record's samples
    pub(crate) samples: [i16; SAMPLES],
}

pub(crate) const FREE: Shape = Shape {
    owner: 0,
    unipolar: false,
    name: [0; NAME_LEN],
    samples: [0; SAMPLES],
};
// FREE only initialises the table (zero: `.bss`). Code never copies it: a copy of a whole shape would be a `.rodata` object, which the packer refuses past its alignment
// (the arena's alignment is 16). A shape is freed by clearing `owner` alone: every reader tests it first.
pub(crate) static mut SHAPES: [Shape; MAX_SHAPES] = [FREE; MAX_SHAPES];

pub(crate) fn slot(number: u32) -> Option<usize> {
    crate::route::shape_index(number)
}

/// The table index of `number` when a module keeps a shape there.
pub(crate) fn kept(t: &[Shape; MAX_SHAPES], number: u32) -> Option<usize> {
    match slot(number) {
        Some(i) if t[i].owner != 0 => Some(i),
        _ => None,
    }
}

/// The samples of the shape with wave number `number`, if a module keeps it.
pub fn shape(number: u32) -> Option<[i16; SAMPLES]> {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map(|i| t[i].samples)
}

/// The owner (module record index + 1) of wave number `number`; 0 when no module keeps it.
pub fn owner_of(number: u32) -> u8 {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map_or(0, |i| t[i].owner)
}

/// The name of the shape with wave number `number` (4 characters), if a module keeps it.
pub fn name_of(number: u32) -> Option<[u8; NAME_LEN]> {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map(|i| t[i].name)
}

/// The highest wave number a loaded shape has (0 when none is loaded).
///
/// The only writers (`Records::apply`, `Provider::detach`) and the readers (the WAVE maximum, the name formatter) run in the UI task, so no reader sees half of a change.
pub fn highest() -> u32 {
    let t = unsafe { &*addr_of!(SHAPES) };
    let mut hi = 0;
    let mut i = 0;
    while i < MAX_SHAPES {
        if t[i].owner != 0 {
            hi = RANGE.lo + i as u32;
        }
        i += 1;
    }
    hi
}

/// Writes the name of the shape that the WAVE parameter value `value` (`wave << 8`; the stock formatter ignores the low byte) names, `width` characters at most (a name stops at
/// its first NUL), then a NUL, and returns the number of characters. A wave number that no module keeps but that is at or below [`highest`] (a gap the sticky allocator can
/// leave, or one above the highest loaded one) gets `width` dashes ("----", "---" on the three-character formatter): the LFO plays the sine for it, so the page says the number is
/// empty. 0 when no shape is loaded at all or the number is outside the range: nothing is written and the stock formatter runs. `out` has room for `width + 1` bytes.
///
/// # Safety
/// `out` is valid for `width + 1` bytes.
pub unsafe fn name_into(value: i32, out: *mut u8, width: usize) -> usize {
    let wave = (value >> 8) as u32;
    let name = match name_of(wave) {
        Some(n) => n,
        None => {
            if wave < RANGE.lo || wave > RANGE.hi || highest() == 0 {
                return 0;
            }
            [b'-'; NAME_LEN]
        }
    };
    let cap = if width < NAME_LEN { width } else { NAME_LEN };
    let mut n = 0;
    while n < cap && name[n] != 0 {
        *out.add(n) = name[n];
        n += 1;
    }
    *out.add(n) = 0;
    n
}

/// Is the shape with wave number `number` unipolar (false for a number no module keeps: the fallback is the sine)?
pub fn unipolar_of(number: u32) -> bool {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map_or(false, |i| t[i].unipolar)
}

/// Shapes in the table.
pub fn used() -> usize {
    let t = unsafe { &*addr_of!(SHAPES) };
    let mut n = 0;
    let mut i = 0;
    while i < MAX_SHAPES {
        if t[i].owner != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// Shapes that module record `owner - 1` keeps (0 when `owner` is 0: no live job runs).
pub(crate) fn own_shapes(owner: u8) -> usize {
    let t = unsafe { &*addr_of!(SHAPES) };
    let mut n = 0;
    let mut i = 0;
    while owner != 0 && i < MAX_SHAPES {
        if t[i].owner == owner {
            n += 1;
        }
        i += 1;
    }
    n
}

/// The value of 32 samples at a 32-bit phase, as signed Q31: the sample at the top 5 bits and the next, linearly interpolated with the next 15 bits (integer
/// arithmetic only; the product of the difference and the 15-bit fraction fits 32 bits). The wave stubs of `core/lfo-table` give the same value from their cells; the graph
/// takes its pictures from this one.
///
/// No panic path: no bounds check and no overflow check. Both indexes are below `SAMPLES` by construction (`phase >> 27` is at most 31, and `j` is masked), and the
/// arithmetic wraps.
#[inline]
pub fn interp(s: &[i16; SAMPLES], phase: u32) -> i32 {
    let i = (phase >> 27) as usize;
    let j = (i + 1) & (SAMPLES - 1);
    let f = ((phase >> 12) & 0x7fff) as i32;
    // SAFETY: i <= 31 and j <= 31, `SAMPLES` is 32
    let (a, b) = unsafe { (*s.get_unchecked(i) as i32, *s.get_unchecked(j) as i32) };
    (a << 16).wrapping_add(b.wrapping_sub(a).wrapping_mul(f).wrapping_shl(1))
}

/// The owner (module record index + 1) of table slot `i`; 0 when it is free. `i < MAX_SHAPES`.
pub(crate) fn owner_at(i: usize) -> u8 {
    let t = unsafe { &*addr_of!(SHAPES) };
    t[i].owner
}

/// Write slot `i`: the shape of module `owner` with `name` and `samples`. `i < MAX_SHAPES`. The owner is written last: every reader tests it first.
pub(crate) fn put(
    i: usize,
    owner: u8,
    unipolar: bool,
    name: [u8; NAME_LEN],
    samples: &[i16; SAMPLES],
) {
    let t = unsafe { &mut *addr_of_mut!(SHAPES) };
    t[i].unipolar = unipolar;
    t[i].name = name;
    t[i].samples = *samples;
    t[i].owner = owner;
}

/// Free slot `i` (clear its owner; every reader tests it first). `i < MAX_SHAPES`.
pub(crate) fn free(i: usize) {
    let t = unsafe { &mut *addr_of_mut!(SHAPES) };
    t[i].owner = 0;
}
