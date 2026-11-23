//! `LinkRows`: the addresses of the other image modules' functions that the package uses. The package links to nothing by symbol, so what the resident object links by
//! name (patchkit's `[object] optional`, 0 when the module is not in the image) comes to it as rows: the imports another module provides (`core/ui`, `core/input`,
//! `core/usb-bulk`, `core/dsp-link`, `core/sdhc-large`) in the order of `optional::table` of `dkrt_sdk::optional_imports!`, and the symbols of `core/dsp-link` and
//! `core/dyndsp` the DSP loader calls. A row is 0 when its module is not in the image; a host test has null rows except the two it provides itself.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinkRows {
    /// The imports another module provides, in the order of `dkrt_sdk::optional_imports!`'s table.
    pub optional: [*const (); dkrt_sdk::OPTIONAL_COUNT],
    /// `core/dsp-link`: `i32 dl_send(u32 dst, const u8 *buf, u32 len)`.
    pub dl_send: *const (),
    /// `core/dsp-link`: `i32 dl_send_win(u32 win, u32 dst, const u32 *buf, u32 len)`.
    pub dl_send_win: *const (),
    /// `core/dsp-link`: `i32 dl_poll(DlStatus *)`.
    pub dl_poll: *const (),
    /// `core/dyndsp`: the info table of the DSP side (`dyndsp_info` is a table, not a function: its address is the table).
    pub dyndsp_info: *const (),
    /// `core/dsp-link`: `void dl_stats_hook(usize feed)`, the feed of the stats pages from `dl_tick`.
    pub dl_stats_hook: *const (),
    /// `core/dsp-link`: `void dl_tick_cb_drop(u32 start, u32 end)`, clears the per-block callback that points into a range.
    pub dl_tick_cb_drop: *const (),
}

// The addresses name functions and tables of the image for the whole run of the system; the rows are built once and only read.
unsafe impl Sync for LinkRows {}
