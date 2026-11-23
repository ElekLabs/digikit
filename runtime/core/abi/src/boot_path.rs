//! Which selection index a file takes: the hash of a store path and the choice of an index for a run-time load.
use dkrt_sdk::{E_SEL_COUNT, SEL_MAX};

/// The hash of a store path (the bytes up to the NUL): FNV-1a, never 0.
#[inline]
pub unsafe fn path_hash(path: *const u8) -> u32 {
    let n = crate::mem::cstr_len(path, 64);
    let h = dkneeds::keys::name_hash_bytes(core::slice::from_raw_parts(path, n));
    if h == 0 {
        1
    } else {
        h
    }
}

/// The selection index a run-time load of the file with path hash `hash` takes (docs/plans/extension-model.md E5c): the index that path had at this boot, when no module lives
/// there now (`live`); else the first index below `count` that no module lives at and no boot path owns (an index an earlier live load added); else a new index (`count`).
/// `E_SEL_COUNT` when all `SEL_MAX` are taken. An unloaded boot module keeps its index for itself.
#[inline]
pub fn pick_index(
    hash: u32,
    boot: &[u32; SEL_MAX],
    count: usize,
    live: &[bool; SEL_MAX],
) -> Result<usize, i32> {
    let count = count.min(SEL_MAX);
    let mut k = 0;
    while k < count {
        if !live[k] && hash != 0 && boot[k] == hash {
            return Ok(k);
        }
        k += 1;
    }
    k = 0;
    while k < count {
        if !live[k] && boot[k] == 0 {
            return Ok(k);
        }
        k += 1;
    }
    if count < SEL_MAX {
        Ok(count)
    } else {
        Err(E_SEL_COUNT)
    }
}

#[cfg(test)]
mod tests;
