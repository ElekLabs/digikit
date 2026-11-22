//! `ApiFns`: the exports of `api.rs` the package tells things. `card_busy` and `poll_drop_range` are the owner's own `extern "C"` functions.
use crate::api;
use dkrt_abi::resident::{ApiFns, Group};

unsafe extern "C" fn notify_unload(owner: u32, base: u32, size: u32) {
    api::notify_unload(owner as usize, base, size)
}

pub(super) static FNS: Group<ApiFns> = Group::new(ApiFns { card_busy: api::card_busy, notify_unload, poll_drop_range: api::poll_drop_range });
