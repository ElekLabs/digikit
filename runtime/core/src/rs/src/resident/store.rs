//! `StoreFns`: the module store, read side.
use crate::{card, store};
use dkrt_abi::resident::StoreFns;
use dkrt_abi::storage::StoreFile;
use dkrt_sdk::{DkrtStoreEntry, CARD_TOO_SMALL};

unsafe extern "C" fn find(path: *const u8, out: *mut StoreFile) -> i32 {
    match store::find(path) {
        Ok(f) => {
            *out = f;
            0
        }
        Err(e) => e,
    }
}
unsafe extern "C" fn load_to_stage(file: *const StoreFile) -> i32 {
    store::load_to_stage(&*file)
}
unsafe extern "C" fn stage() -> *mut u8 {
    store::stage()
}
unsafe extern "C" fn list(prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
    store::list(prefix, out, max)
}
unsafe extern "C" fn find_name(prefix: *const u8, name: *const u8, len: u32, out: *mut u8) -> i32 {
    store::find_name(prefix, core::slice::from_raw_parts(name, len as usize), out as *mut [u8; 64])
}
unsafe extern "C" fn read_sector(rel: u32, out: *mut u8) -> i32 {
    store::read_sector(rel, out)
}
/// The store's area in KiB (0 when the card is too small for it) and what the headers say is used (a card with no header: nothing).
unsafe extern "C" fn capacity(out: *mut u32) {
    *out = if card::state() == CARD_TOO_SMALL { 0 } else { store::layout().limit() / 2 };
    *out.add(1) = match store::seen() {
        Some((used, _)) => used,
        None => 0,
    };
}

pub(super) static FNS: StoreFns = StoreFns { find, load_to_stage, stage, list, find_name, read_sector, capacity };
