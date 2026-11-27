//! The parameter blocks of `rows_info`: the rows the image reserved in the parameter table, one block for each owner the device file names (`[[rows.block]]`).
use crate::rows_info::w;
use dkrt_abi::mem::iw;

/// One block: the code of its first row, how many rows it has and the stock code whose row a free row copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub first: u32,
    pub rows: u32,
    pub template: u32,
}

/// The block of owner number `owner` in the table `inf` (`rows_info`), or none: the table has no block of that number.
///
/// # Safety
/// `inf` is a table that `rows_info` returned.
pub unsafe fn block(inf: *const u32, owner: u32) -> Option<Block> {
    let n = iw(inf, w::BLOCKS) as usize;
    let mut i = 0;
    while i < n {
        let at = w::BLOCK_0 + 4 * i;
        if iw(inf, at) == owner {
            return Some(Block {
                first: iw(inf, at + 1),
                rows: iw(inf, at + 2),
                template: iw(inf, at + 3),
            });
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows_info;
    use dkrt_test_support::FakeCore;

    /// A block is found by its owner number, not by its place; an owner the table does not have gives none.
    #[test]
    fn a_block_is_found_by_its_owner_number() {
        let mut core = FakeCore::new();
        let mut t = std::vec![0u32; w::WORDS];
        t[w::BLOCKS] = 2;
        // owner, first code, rows, template
        t[w::BLOCK_0..w::BLOCK_0 + 8].copy_from_slice(&[7, 100, 4, 0x75, 3, 120, 2, 0x44]);
        core.set_rows_info(t);
        unsafe {
            let inf = rows_info(&core);
            assert_eq!(
                block(inf, 3),
                Some(Block {
                    first: 120,
                    rows: 2,
                    template: 0x44
                })
            );
            assert_eq!(block(inf, 7).map(|b| b.first), Some(100));
            assert_eq!(block(inf, 1), None);
        }
    }
}
