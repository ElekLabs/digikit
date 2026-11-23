//! `StoreFns`: the module store on the card (`storage/store.rs`), read side.
use crate::storage::StoreFile;
use dkrt_sdk::DkrtStoreEntry;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StoreFns {
    /// Find the file at the store path: 0 and its place in `*out`, or an `E_*` code.
    pub find: unsafe extern "C" fn(path: *const u8, out: *mut StoreFile) -> i32,
    /// Read a found file into the staging buffer: 0 or an `E_*` code.
    pub load_to_stage: unsafe extern "C" fn(file: *const StoreFile) -> i32,
    /// The staging buffer.
    pub stage: unsafe extern "C" fn() -> *mut u8,
    pub list: unsafe extern "C" fn(prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32,
    /// The entry `name` of the folder `prefix`: the 64 bytes of its table entry in `out`.
    pub find_name:
        unsafe extern "C" fn(prefix: *const u8, name: *const u8, len: u32, out: *mut u8) -> i32,
    pub read_sector: unsafe extern "C" fn(rel: u32, out: *mut u8) -> i32,
    /// The area of the store and what is used, in KiB: `out[0]` and `out[1]` (0 and 0 when the card has no store).
    pub capacity: unsafe extern "C" fn(out: *mut u32),
}
