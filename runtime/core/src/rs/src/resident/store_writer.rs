//! `StoreWriterFns`: the write side of the store.
use crate::store_writer;
use dkrt_abi::resident::StoreWriterFns;

extern "C" fn is_open() -> bool {
    store_writer::is_open()
}
extern "C" fn is_locked() -> bool {
    crate::storage::lock::is_locked()
}
extern "C" fn retire(owner: u32) {
    store_writer::retire(owner as u8)
}

pub(super) static FNS: StoreWriterFns = StoreWriterFns { is_open, is_locked, retire };
