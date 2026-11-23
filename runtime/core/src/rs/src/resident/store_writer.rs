//! `StoreWriterFns`: the write side of the store. `is_open` and `is_locked` are the owners' own `extern "C"` functions.
use crate::store_writer;
use dkrt_abi::resident::{Group, StoreWriterFns};

extern "C" fn retire(owner: u32) {
    store_writer::retire(owner as u8)
}

pub(super) static FNS: Group<StoreWriterFns> =
    Group::new(StoreWriterFns { is_open: store_writer::is_open, is_locked: crate::storage::lock::is_locked, retire });
