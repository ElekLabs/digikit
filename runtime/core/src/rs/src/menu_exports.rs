//! The exports the DIGIKIT menu calls (`dkrt_scan`, `dkrt_fit`, `dkrt_capacity`, `dkrt_live` and their companions): the symbols stay in the resident core while the menu
//! is not part of the package, and each one calls the package through its hook (`hooks.rs`). With no package they answer with the defaults of the hook.

use crate::hooks;
use crate::state::init_once;
use dkrt_sdk::{DkrtCap, DkrtFitOut, DkrtLive, DkrtScan};

/// `dkrt_scan(out, max)`: the `.dkm` files of the store under `modules/` (docs in `abi.toml`).
#[no_mangle]
pub unsafe extern "C" fn dkrt_scan(out: *mut DkrtScan, max: u32) -> i32 {
    init_once();
    hooks::scan(out, max)
}

/// `dkrt_scan_reads()`: the sector reads the last `dkrt_scan` made to read the needs.
#[no_mangle]
pub unsafe extern "C" fn dkrt_scan_reads() -> u32 {
    hooks::scan_reads()
}

/// `dkrt_fit(entries, n, ticked, out)`: which modules fit.
#[no_mangle]
pub unsafe extern "C" fn dkrt_fit(entries: *const DkrtScan, n: u32, ticked: *const u8, out: *mut DkrtFitOut) -> i32 {
    init_once();
    hooks::fit(entries, n, ticked, out)
}

/// `dkrt_capacity(out, max)`: the entries of the CAPACITY command.
#[no_mangle]
pub unsafe extern "C" fn dkrt_capacity(out: *mut DkrtCap, max: u32) -> u32 {
    init_once();
    hooks::capacity(out, max)
}

/// `dkrt_live(op, path)`: queue a live job from the menu.
#[no_mangle]
pub unsafe extern "C" fn dkrt_live(op: u32, path: *const u8) -> i32 {
    init_once();
    hooks::live(op, path)
}

/// `dkrt_live_state(out)`: the phase, result, step and teardown flag of the live job.
#[no_mangle]
pub unsafe extern "C" fn dkrt_live_state(out: *mut DkrtLive) {
    hooks::live_state(out)
}
