//! core/runtime: the digikit runtime (dkrt). Code, data and the module arena live in `cf.expansion`.
//!
//! What it does (docs/plans/digikit-runtime.md, docs/design/digikit-runtime.md): reads `.dkm` module files from the card
//! module store with the firmware's own block read, checks them, copies and relocates them into a private arena, flushes
//! the caches and calls `init`. A hook in the UI task's event loop (`dkrt_tick`) polls a mailbox for host commands (the selection, the restart)
//! and calls the `tick` of every loaded module.
//!
//! Rules: task context only (never from an interrupt); no `core::fmt`; every `extern "C"` function takes and returns
//! integers or pointers. Built with rustc_codegen_gcc for -mcpu=54418 (finding 26).
#![cfg_attr(not(test), no_std)]
#![allow(static_mut_refs)]

#[cfg(not(test))]
use core::panic::PanicInfo;
use core::ptr::{addr_of, read_volatile, write_volatile};
use dkrt_sdk::*;

mod api;
mod arena;
mod dev;
pub mod core_api;
pub mod collect;
mod handlers;
pub mod storage;
pub(crate) use storage::storage_adapter;
pub use storage::card::{block_write, block_write_with};
pub use storage::{card, store, store_writer};
#[rustfmt::skip]
mod gen_services;
pub mod boot;
pub mod fault;
pub mod dlog;
mod dsp;
mod link;
pub mod dsp_stats;
pub mod entry;
mod gen_abi_check;
mod resident_query;
mod loader;
pub mod registry;
pub mod scan;
pub mod settings;
mod settings_overlay;
mod settings_reload;
mod files;
mod hash32;
// The file is not aux.rs: Windows does not allow a file named AUX, with any extension.
#[path = "aux_task.rs"]
pub mod aux;
pub mod speed;
mod intro;
mod safe_screen;
mod sha256;
mod io;
mod invariants;
mod watch;
mod worker;
mod health;
mod imports;
mod svc_imports;
mod svc_rows;
mod mailbox;
mod admission;
mod platform;
pub mod reload;
mod safe_mode;
mod selection;
mod report;
mod state;
mod tick;
mod transport;
mod store_cmds;
mod admin_cmds;
mod module_cmds;
mod module_ui;

// The items that moved out of this file are still reachable as `crate::name`.
pub use api::*;
pub use health::*;
pub use imports::*;
pub use platform::*;
pub use report::*;
pub use resident_query::dkrt_service_query;
pub use state::*;
pub use tick::*;

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    crate::fault::panic()
}

extern "C" {
    /// The s16 words of a Sound array on this device (`[fx] sound_words`).
    static dkrt_sound_words: u32;
}

/// Vtable slot 0x28 of a Sound value object: the Sound record, whose s16 parameter words start at +0x14 (`Value<sound_struct>::vfunc_10`: the word at +0x10 of the object).
const SOUND_VT_RECORD: u32 = 0x28;
const SOUND_WORDS_AT: u32 = 0x14;

/// Import `dk_selected_track`: the selected track, or -2 when there is no project yet. The same read as `debug_selected_params` (`platform::selected_sound`).
pub(crate) unsafe extern "C" fn selected_track() -> i32 {
    match platform::selected_sound() {
        Some((track, _)) => track as i32,
        None => -2,
    }
}

/// Import `dk_debug_selected_params`: the selected track (`view_track` of the project's settings) and the first `n` words of its Sound array, through the firmware's
/// own accessors, plain reads. The words are big-endian s16 at a byte address that may be odd on the Digitone II (stride 0x48b), so they are read byte by byte.
/// -2: no project yet; -3: the track has no record (a kit is loading): `out` is not written.
pub(crate) unsafe extern "C" fn debug_selected_params(out: *mut i16, n: u32) -> i32 {
    let Some((track, sound)) = platform::selected_sound() else { return -2 };
    let vt = read_volatile(sound as *const u32);
    let record: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(read_volatile((vt + SOUND_VT_RECORD) as *const u32) as usize);
    let rec = record(sound);
    if rec == 0 {
        return -3;
    }
    let words = read_volatile(addr_of!(dkrt_sound_words));
    let mut i = 0;
    while i < n {
        let v = if i < words { dkrt_abi::mem::rd16(rec + SOUND_WORDS_AT + 2 * i) } else { 0 };
        write_volatile(out.add(i as usize), v as i16);
        i += 1;
    }
    track as i32
}



mod lifecycle;
#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod test_lock;
#[cfg(test)]
mod fake_platform;
#[cfg(test)]
mod spans_tests;
#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod svc_contract_tests;
#[cfg(test)]
mod fake_storage;
#[cfg(test)]
mod host_io;
#[cfg(test)]
mod store_tests;


