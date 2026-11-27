//! The index of a wave number in the shape table. The hooks of the LFO tick (`dks/src/hooks-*.s`) run on the cycles where the clamped wave is 6, read the raw wave byte again and test it
//! against [`RANGE`] inline (the tick is an interrupt and a stock wave must not pay for a call): 7 to 14 is a shape, below it and above it stock code. `tests/route.rs` reads the two
//! assembler files and asserts that their bounds are `RANGE`.
use crate::records::{MAX_SHAPES, RANGE};

/// The index into the shape table of the wave number `number`: `None` outside `RANGE`. The one place that maps a number to a cell of the table (`eval` reads it too).
#[inline]
pub fn shape_index(number: u32) -> Option<usize> {
    const _: () = assert!(RANGE.hi - RANGE.lo + 1 == MAX_SHAPES as u32);
    let idx = number.wrapping_sub(RANGE.lo);
    if idx < MAX_SHAPES as u32 {
        Some(idx as usize)
    } else {
        None
    }
}
