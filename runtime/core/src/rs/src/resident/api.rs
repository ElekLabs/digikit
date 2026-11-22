//! `ApiFns`: the exports of `api.rs` the package tells things.
use crate::api;
use dkrt_abi::resident::ApiFns;

unsafe extern "C" fn card_busy() -> bool {
    api::card_busy()
}
unsafe extern "C" fn notify_unload(owner: u32, base: u32, size: u32) {
    api::notify_unload(owner as usize, base, size)
}
unsafe extern "C" fn poll_drop_range(base: u32, size: u32) {
    api::poll_drop_range(base, size)
}

pub(super) static FNS: ApiFns = ApiFns { card_busy, notify_unload, poll_drop_range };
