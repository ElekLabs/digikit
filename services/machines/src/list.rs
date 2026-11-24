//! The machine list the firmware's list builder copies, and the rank pairs its sort comparator takes (RC4-3; they were `mf_ensure` of the image point).
//!
//! The firmware builds the list of machine types once, from a buffer of types: the stock order with every entry of the registry inserted at its `position` (a position
//! above the length is the end; position `0xffff_ffff` is a gap row, a number nobody selected, which the list does not show). `start` builds both tables here; the four
//! patches of the service point the builder at `DKRT_MF_LIST` and size it, and the hook of the sort's insert reads `DKRT_MF_RANK`. A reserved row with no name is a member of
//! the list as well: the image's copy leaves it out of what the caller gets until a module fills it.
//! `build` is pure; `from_image` reads the rows through the core.
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;

/// The most machine types the list holds: the registry's `max` of 32 and the stock types.
pub const MAX_TYPES: usize = 64;
/// A row whose position is this one is a gap row: not listed.
pub const GAP: u32 = 0xffff_ffff;

/// The list buffer the list builder reads (`list.operands` patch), one type per word.
#[no_mangle]
pub static mut DKRT_MF_LIST: [u32; MAX_TYPES] = [0; MAX_TYPES];
/// The (type, display position) pairs the sort's insert takes (the rank hook), 8 bytes each.
#[no_mangle]
pub static mut DKRT_MF_RANK: [[u32; 2]; MAX_TYPES] = [[0; 2]; MAX_TYPES];
/// The number of pairs in `DKRT_MF_RANK`.
#[no_mangle]
pub static mut DKRT_MF_RANKN: u32 = 0;

/// The displayed order: `stock` with the type `first + k` inserted at `positions[k]` (clamped to the length so far); a gap row is left out. Writes the order to `list`
/// and the pairs `(type, index)` to `rank`; returns the length of the order. `list` and `rank` hold at least `stock.len() + positions.len()` items.
pub fn build(
    stock: &[u32],
    positions: &[u32],
    first: u32,
    list: &mut [u32],
    rank: &mut [[u32; 2]],
) -> usize {
    let mut len = stock.len();
    list[..len].copy_from_slice(stock);
    for (k, &pos) in positions.iter().enumerate() {
        if pos == GAP {
            continue;
        }
        let at = (pos as usize).min(len);
        let mut i = len;
        while i > at {
            list[i] = list[i - 1];
            i -= 1;
        }
        list[at] = first + k as u32;
        len += 1;
    }
    for i in 0..len {
        rank[i] = [list[i], i as u32];
    }
    len
}

/// Read the stock order and the rows' positions from the image and build the tables of this crate. Returns the number of listed types, or `None` when the image gives
/// no registry, or one that does not fit.
///
/// # Safety
/// `stock` is the address of `first` words (the stock order), `rows` of `count` rows of `row_size` bytes with the position at byte `o_pos`.
pub unsafe fn from_image<C: CoreApi>(
    core: &C,
    stock: u32,
    first: u32,
    count: u32,
    rows: u32,
    row_size: u32,
    o_pos: u32,
) -> Option<usize> {
    let (first_n, count_n) = (first as usize, count as usize);
    if stock == 0 || first_n + count_n > MAX_TYPES {
        return None;
    }
    let mut s = [0u32; MAX_TYPES];
    let mut p = [0u32; MAX_TYPES];
    for (i, w) in s[..first_n].iter_mut().enumerate() {
        *w = core.rd(stock + 4 * i as u32);
    }
    for (k, w) in p[..count_n].iter_mut().enumerate() {
        *w = core.rd(rows + k as u32 * row_size + o_pos);
    }
    let list = &mut *addr_of_mut!(DKRT_MF_LIST);
    let rank = &mut *addr_of_mut!(DKRT_MF_RANK);
    let n = build(&s[..first_n], &p[..count_n], first, list, rank);
    core::ptr::write_volatile(addr_of_mut!(DKRT_MF_RANKN), n as u32);
    Some(n)
}

/// The address of the list buffer and its size in bytes for `types` entries (the values of the list patches).
pub fn list_address() -> u32 {
    addr_of!(DKRT_MF_LIST) as u32
}

#[cfg(test)]
mod tests;
