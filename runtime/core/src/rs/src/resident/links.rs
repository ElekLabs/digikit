//! `LinkRows`: the other image modules' functions, which only the resident object links by name (`[object] optional` of the module.toml: each symbol is 0 when its module is not
//! in the image). The package gets their addresses here, in the order of its `optional::table`.
use dkrt_abi::resident::{Group, LinkRows};

// The system package's imports of other modules (`dk_ui_*`, `dk_input_*`, `dk_usb_bulk_*`, `dk_dsp_link_*`, `dk_sdhc_large_*`).
dkrt_sdk::optional_links!();

extern "C" {
    fn dl_stats_hook();
    fn dl_tick_cb_drop();
}

#[cfg(not(test))]
extern "C" {
    fn dl_send();
    fn dl_send_win();
    fn dl_poll();
    fn dyndsp_info();
}

/// The address of a symbol only the device image has: a host test links none of them, so its row is null.
macro_rules! device_row {
    ($f:ident) => {{
        #[cfg(test)]
        {
            core::ptr::null()
        }
        #[cfg(not(test))]
        {
            $f as *const ()
        }
    }};
}

pub(super) static ROWS: Group<LinkRows> = Group::new(LinkRows {
    optional: optional_link::ADDRESSES,
    dl_send: device_row!(dl_send),
    dl_send_win: device_row!(dl_send_win),
    dl_poll: device_row!(dl_poll),
    dyndsp_info: device_row!(dyndsp_info),
    // the host tests give these two themselves (`package/cleanup_tests.rs`)
    dl_stats_hook: dl_stats_hook as *const (),
    dl_tick_cb_drop: dl_tick_cb_drop as *const (),
});
