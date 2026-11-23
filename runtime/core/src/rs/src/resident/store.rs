//! `StoreFns`: the module store, read side. `stage`, `list` and `read_sector` are the owner's own `extern "C"` functions.
use crate::{card, store};
use dkrt_abi::resident::{Group, StoreFns};
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
unsafe extern "C" fn find_name(prefix: *const u8, name: *const u8, len: u32, out: *mut u8) -> i32 {
    store::find_name(prefix, core::slice::from_raw_parts(name, len as usize), out as *mut [u8; 64])
}
/// The store's area in KiB (0 when the card is too small for it) and what the headers say is used (a card with no header: nothing).
unsafe extern "C" fn capacity(out: *mut u32) {
    *out = if card::state() == CARD_TOO_SMALL { 0 } else { store::layout().limit() / 2 };
    *out.add(1) = match store::seen() {
        Some((used, _)) => used,
        None => 0,
    };
}

pub(super) static FNS: Group<StoreFns> = Group::new(StoreFns { find, load_to_stage, stage: store::stage, list: store::list, find_name, read_sector: store::read_sector, capacity });
