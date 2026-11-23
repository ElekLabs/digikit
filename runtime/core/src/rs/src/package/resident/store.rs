//! The module store on the card, read side.
use super::table;
use dkrt_abi::storage::StoreFile as File;
use dkrt_sdk::DkrtStoreEntry;

#[inline(always)]
pub(crate) unsafe fn find(path: *const u8) -> Result<File, i32> {
    let mut f = File { first: 0, size: 0, sha: [0; 32] };
    let r = (table().store.find)(path, &mut f);
    if r == 0 {
        Ok(f)
    } else {
        Err(r)
    }
}

#[inline(always)]
pub(crate) unsafe fn load_to_stage(f: &File) -> i32 {
    (table().store.load_to_stage)(f)
}

/// The staging buffer.
#[inline(always)]
pub(crate) unsafe fn stage() -> *mut u8 {
    (table().store.stage)()
}

#[inline(always)]
pub(crate) unsafe fn list(prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
    (table().store.list)(prefix, out, max)
}

#[inline(always)]
pub(crate) unsafe fn find_name(prefix: *const u8, name: &[u8], out: *mut [u8; 64]) -> i32 {
    (table().store.find_name)(prefix, name.as_ptr(), name.len() as u32, out as *mut u8)
}

#[inline(always)]
pub(crate) unsafe fn read_sector(rel: u32, out: *mut u8) -> i32 {
    (table().store.read_sector)(rel, out)
}

/// The area of the store and what is used, in KiB.
#[inline(always)]
pub(crate) unsafe fn capacity() -> (u32, u32) {
    let mut out = [0u32; 2];
    (table().store.capacity)(out.as_mut_ptr());
    (out[0], out[1])
}
