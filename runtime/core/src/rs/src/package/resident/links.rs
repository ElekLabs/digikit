//! The other image modules' functions, as the resident core links them (`dkrt_abi::resident::LinkRows`): the addresses are 0 when their module is not in the image.
use super::table;

/// The imports another module provides, in the order of `optional::table`.
#[inline(always)]
pub(crate) fn optional() -> &'static [*const (); dkrt_sdk::OPTIONAL_COUNT] {
    &table().links.optional
}

/// `core/dsp-link` `dl_send`: its address, or 0.
#[inline(always)]
pub(crate) fn dl_send() -> usize {
    core::hint::black_box(table().links.dl_send as usize)
}

/// `core/dsp-link` `dl_send_win`: its address, or 0.
#[inline(always)]
pub(crate) fn dl_send_win() -> usize {
    core::hint::black_box(table().links.dl_send_win as usize)
}

/// `core/dsp-link` `dl_poll`: its address, or 0.
#[inline(always)]
pub(crate) fn dl_poll() -> usize {
    core::hint::black_box(table().links.dl_poll as usize)
}

/// `core/dyndsp`'s info table: its address, or 0.
#[inline(always)]
pub(crate) fn dyndsp_info() -> usize {
    core::hint::black_box(table().links.dyndsp_info as usize)
}

/// `core/dsp-link` `dl_stats_hook`: its address, or 0.
#[inline(always)]
pub(crate) fn dl_stats_hook() -> usize {
    core::hint::black_box(table().links.dl_stats_hook as usize)
}

/// `core/dsp-link` `dl_tick_cb_drop`: its address, or 0.
#[inline(always)]
pub(crate) fn dl_tick_cb_drop() -> usize {
    core::hint::black_box(table().links.dl_tick_cb_drop as usize)
}
