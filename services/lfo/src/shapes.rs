//! `lfo` service, the shape table the LFO tick reads and the pure functions over it: the cells of a shape (the sample as Q31 and the step to the next sample), the lookups of
//! the UI task (`shape`, `name_of`, `highest`, ...), the interpolation and `eval`, the lookup the interrupt site of the LFO tick calls. The table is a named static: the tick
//! reads it in an interrupt and the hooks of the file reach it by symbol (docs/plans/rc3-briefs.md D7); the service writes it under the interrupt mask.
use crate::records::{MAX_SHAPES, NAME_LEN, RANGE, SAMPLES};
use core::ptr::addr_of;

/// One cell of a shape, as the tick reads it: the sample as Q31 and the step to the next sample (the next of sample 31 is sample 0). The step needs 17 bits, so it is an `i32`.
/// 4-byte aligned on purpose: the ColdFire GCC gives an `i32` an alignment of 2, which would put the cells of a shape at an offset of 6 and make the tick read them misaligned.
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub(crate) struct Cell {
    /// `samples[i] << 16`
    pub(crate) v: i32,
    /// `samples[(i + 1) & 31] - samples[i]`
    pub(crate) d: i32,
}

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
    /// the delta table, built by `apply_record` from the record's samples
    pub(crate) cells: [Cell; SAMPLES],
}

pub(crate) const FREE: Shape = Shape {
    owner: 0,
    unipolar: false,
    name: [0; NAME_LEN],
    cells: [Cell { v: 0, d: 0 }; SAMPLES],
};
// FREE only initialises the table (zero: `.bss`). Code never copies it: a copy of a whole shape would be a 264-byte `.rodata` object aligned to 32, which the packer refuses
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
    kept(t, number).map(|i| {
        let mut out = [0i16; SAMPLES];
        let mut k = 0;
        while k < SAMPLES {
            out[k] = (t[i].cells[k].v >> 16) as i16;
            k += 1;
        }
        out
    })
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
/// Not read under the interrupt mask, and no torn read is possible: `owner` is one byte, and the only writers (`apply_record`, `remove`) change a whole entry under the interrupt
/// mask, so a reader at task level sees each entry before or after a change, never half of one. A change that lands during the loop gives a number that held at some point
/// of the call; the callers (the WAVE maximum and the name formatter, both in the UI task) tolerate that.
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
/// leave) gets `width` dashes ("----", "---" on the three-character formatter): the tick plays TRI for it, so the page says the number is empty. 0 when the number is above
/// the highest loaded one: nothing is written and the stock formatter prints its "ERR". `out` has room for `width + 1` bytes.
///
/// # Safety
/// `out` is valid for `width + 1` bytes.
pub unsafe fn name_into(value: i32, out: *mut u8, width: usize) -> usize {
    let wave = (value >> 8) as u32;
    let name = match name_of(wave) {
        Some(n) => n,
        None => {
            if wave < RANGE.lo || wave > highest() {
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

/// Is the shape with wave number `number` unipolar (false for a number no module keeps: the fallback is TRI)?
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

/// The built-in triangle (the stock TRI function): the fallback of a number whose shape is not loaded. Full scale, 0 at phase 0.
#[inline]
pub fn tri(phase: u32) -> i32 {
    let d = phase.wrapping_add(0x4000_0000);
    let mask = if d >> 31 != 0 { 0xffff_ffffu32 } else { 0 };
    ((d << 1) ^ mask ^ 0x8000_0000) as i32
}

/// The value of 32 samples at a 32-bit phase, as signed Q31: the sample at the top 5 bits and the next, linearly interpolated with the next 15 bits (integer
/// arithmetic only; the product of the difference and the 15-bit fraction fits 32 bits). This is the formula in its plain form, from the samples; `interp_cells` gives
/// the same value from the delta table that the tick reads.
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

/// The delta table of 32 samples: `{ s[i] << 16, s[(i + 1) & 31] - s[i] }` for each `i`.
pub(crate) fn cells_of(s: &[i16; SAMPLES]) -> [Cell; SAMPLES] {
    let mut c = [Cell { v: 0, d: 0 }; SAMPLES];
    let mut i = 0;
    while i < SAMPLES {
        let a = s[i] as i32;
        let b = s[(i + 1) & (SAMPLES - 1)] as i32;
        c[i] = Cell {
            v: a << 16,
            d: b - a,
        };
        i += 1;
    }
    c
}

/// The same value as `interp`, from the delta table: one load of a cell, one multiply, one shift and one add. `d` is at most 17 bits and `f` 15, so the product fits an `i32`.
#[inline]
fn interp_cells(c: &[Cell; SAMPLES], phase: u32) -> i32 {
    let i = (phase >> 27) as usize;
    let f = ((phase >> 12) & 0x7fff) as i32;
    // SAFETY: i <= 31, `SAMPLES` is 32
    let cell = unsafe { c.get_unchecked(i) };
    cell.v.wrapping_add(cell.d.wrapping_mul(f).wrapping_shl(1))
}

/// The value of the wave `number` at `phase`, signed Q31: the shape's samples interpolated, or TRI when no module keeps the number. This is what the
/// interrupt site of the LFO tick calls for a wave above RAND: it reads the table and writes nothing, and it has no panic path (no bounds check: `number - RANGE.lo`
/// is below `MAX_SHAPES` before the table is read).
#[inline]
pub fn eval(number: u32, phase: u32) -> i32 {
    let idx = match crate::route::shape_index(number) {
        Some(i) => i,
        None => return tri(phase),
    };
    // SAFETY: idx < MAX_SHAPES, the length of SHAPES; the table is only read here
    let sh = unsafe { &*(addr_of!(SHAPES) as *const Shape).add(idx) };
    if sh.owner == 0 {
        return tri(phase);
    }
    interp_cells(&sh.cells, phase)
}
