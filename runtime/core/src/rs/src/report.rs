//! What the runtime reports: `dkrt_status`, `dkrt_capacity`, `dkrt_diag` and the `dk_debug_*` imports that read the device.

use crate::platform::{self, device};
use crate::platform::st;
use crate::state::{init_once, MAX_MODULES};
use crate::{arena, card, fault, dlog, settings, speed, watch};
use crate::package::{dsp};
use core::ptr::{addr_of, read_volatile};
use dkrt_abi::platform::Platform;
use dkrt_sdk::*;

extern "C" {
    /// Per-device data from the device file (`[digikit] peek`), defined by `src/cf/<dev>/device.s`: the number of ranges and `lo, hi` pairs.
    static dkrt_peek_n: u32;
    static dkrt_peek_ranges: [u32; 8];
    /// The profiling clock of the device file (`[digikit] span_clock`, `span_clock_hz`): its address (0 when absent) and frequency.
    static dkrt_span_clock: u32;
    static dkrt_span_clock_hz: u32;
}

/// Import `dk_debug_speed_flags`: the run-time switches of the speed levers (`speed::update`).
pub(crate) unsafe extern "C" fn debug_speed_flags(clear: u32, set: u32) -> u32 {
    speed::update(clear, set)
}

/// Import `dk_device_track_count`: the tracks of the device, from its profile (the device object's `tracks`); 0 when the device file gives none.
pub(crate) extern "C" fn device_track_count() -> u32 {
    platform::profile().tracks
}

/// Import `dk_debug_peek`: how many RAM ranges the device file allows PEEK to read; for `i` below that, range `i` as `lo, hi` into `out`.
pub(crate) unsafe extern "C" fn debug_peek(i: u32, out: *mut u32) -> u32 {
    let n = read_volatile(addr_of!(dkrt_peek_n)).min(4);
    if i < n {
        let t = addr_of!(dkrt_peek_ranges) as *const u32;
        *out = read_volatile(t.add(2 * i as usize));
        *out.add(1) = read_volatile(t.add(2 * i as usize + 1));
    }
    n
}

/// Import `dk_debug_span_clock`: the address of the profiling clock (0 when the device has none); its frequency in Hz into `out_hz` when that is not null.
pub(crate) unsafe extern "C" fn debug_span_clock(out_hz: *mut u32) -> u32 {
    let addr = read_volatile(addr_of!(dkrt_span_clock));
    if addr != 0 && !out_hz.is_null() {
        *out_hz = read_volatile(addr_of!(dkrt_span_clock_hz));
    }
    addr
}

/// Import `dk_debug_poll`: the owner and function of poll slot `slot` (`dkrt_poll_add`): 0 when the slot is empty, 1 + the module record, or 0xff for the firmware image.
pub(crate) unsafe extern "C" fn debug_poll(slot: u32, out_fn: *mut u32) -> u32 {
    if slot as usize >= crate::state::POLLS {
        return 0;
    }
    let f = read_volatile(addr_of!(crate::state::DKRT_POLLS[slot as usize]));
    if f == 0 {
        return 0;
    }
    if !out_fn.is_null() {
        *out_fn = f as u32;
    }
    match read_volatile(addr_of!(crate::state::DKRT_POLL_OWNER[slot as usize])) {
        crate::state::POLL_OWNER_RUNTIME => 0xff,
        rec => rec as u32 + 1,
    }
}

/// The stock filter types (`cf.filter.fn.accessor`: a type above 5 reads the shared fallback row); the commit has no check of its own.
const STOCK_FILTER_TYPES: u32 = 6;

/// Import `dk_debug_setup`: set the machine type (`op` 1) or the filter type (`op` 2) of a track through the firmware's own kit commit. `E_RANGE` for an op, a
/// track or a type no chooser offers, else what `Platform::set_track_type` reports. Run it in the UI task.
pub(crate) unsafe extern "C" fn debug_setup(op: u32, track: u32, ty: u32) -> i32 {
    if !setup_args_ok(&platform::profile(), &crate::package::module_cmds::machine_type_loaded, op, track, ty) {
        return E_RANGE;
    }
    crate::platform::PLATFORM.set_track_type(op, track, ty)
}

/// A machine type is one of the device's stock types (`0 .. stock_machine_types`) or a run-time type of a loaded module (`loaded`): the built-in rows between them,
/// the gap rows and the run-time rows nobody loaded are offered by no chooser. A filter type is a stock one.
fn setup_args_ok(p: &dkrt_abi::platform::DeviceProfile, loaded: &dyn Fn(u32) -> bool, op: u32, track: u32, ty: u32) -> bool {
    let ty_ok = match op {
        1 => ty < p.stock_machine_types || (p.is_dyn_type(ty) && loaded(ty)),
        2 => ty < STOCK_FILTER_TYPES,
        _ => false,
    };
    track < 16 && ty_ok
}

#[cfg(test)]
mod setup_tests {
    use super::*;
    use dkrt_abi::profiles;

    #[test]
    fn setup_takes_stock_and_loaded_run_time_machine_types_and_stock_filter_types() {
        for p in [profiles::DT2, profiles::DN2] {
            let loaded = |t: u32| t == p.dyn_type_first + 1;
            let first_dyn = p.dyn_type_first;
            assert!(setup_args_ok(&p, &loaded, 1, 0, 0));
            assert!(setup_args_ok(&p, &loaded, 1, 15, p.stock_machine_types - 1), "the last stock type");
            assert!(!setup_args_ok(&p, &loaded, 1, 0, p.stock_machine_types), "the first row after the stock types is built-in or a gap");
            assert!(!setup_args_ok(&p, &loaded, 1, 0, first_dyn - 1), "a gap row below the run-time range");
            assert!(setup_args_ok(&p, &loaded, 1, 0, first_dyn + 1), "a loaded run-time row");
            assert!(!setup_args_ok(&p, &loaded, 1, 0, first_dyn), "a run-time row nobody loaded");
            assert!(!setup_args_ok(&p, &|_| true, 1, 0, first_dyn + p.dyn_types), "past the run-time range");
            assert!(setup_args_ok(&p, &loaded, 2, 1, 5));
            assert!(!setup_args_ok(&p, &loaded, 2, 1, 6), "a filter type above the stock ones has no row");
            assert!(!setup_args_ok(&p, &loaded, 1, 16, 0), "track 16 does not exist");
            assert!(!setup_args_ok(&p, &loaded, 0, 0, 0) && !setup_args_ok(&p, &loaded, 3, 0, 0), "op is 1 or 2");
        }
        assert_eq!((profiles::DT2.stock_machine_types, profiles::DN2.stock_machine_types), (7, 5));
    }
}

/// Import `dk_debug_track_type`: the machine type of a track of the active kit, from the firmware's own accessors (read-only); -1 past track 15 or when the
/// firmware has no type for it, -2 when there is no project yet.
pub(crate) unsafe extern "C" fn debug_track_type(track: u32) -> i32 {
    match crate::platform::PLATFORM.track_type(track) {
        None => -2,
        Some(t) => t,
    }
}



/// Fill `out` (a `DkrtStatus`).
#[no_mangle]
pub unsafe extern "C" fn dkrt_status(out: *mut DkrtStatus) {
    init_once();
    let s = st();
    let (free, largest) = arena::stats();
    let mut n = 0;
    let mut i = 0;
    while i < MAX_MODULES {
        if s.modules[i].used != 0 {
            n += 1;
        }
        i += 1;
    }
    let o = &mut *out;
    o.magic = s.magic;
    o.runtime = s.runtime;
    o.device = device();
    o.arena_base = s.arena_lo;
    o.arena_size = s.arena_hi - s.arena_lo;
    o.arena_free = free;
    o.arena_largest = largest;
    o.modules = n;
    o.ticks = s.ticks;
    o.last_result = s.last_result;
    o.loads = s.loads;
    o.tick_calls = s.tick_calls;
    o.state = dsp::state();
    o.dsp_size = dsp::area_bytes();
    let (dfree, dlargest) = dsp::area_stats();
    o.dsp_free = dfree;
    o.dsp_largest = dlargest;
}

/// `dkrt_diag(out)`: what the debug log, the card check and the DSP watch found (a `DkrtDiag`).
#[no_mangle]
pub unsafe extern "C" fn dkrt_diag(out: *mut DkrtDiag) {
    init_once();
    let o = &mut *out;
    let c = card::info();
    let (dsp_ok, hb) = watch::heartbeat();
    let k = fault::seen();
    *o = DkrtDiag {
        magic: DIAG_MAGIC,
        version: 1,
        card_state: c.state,
        card_first_nonblank: c.first_nonblank,
        card_hash: c.hash,
        card_refused: c.refused,
        card_read_errors: c.read_errors,
        debug_on: dlog::sink_present() as u32,
        log_oldest: 0,
        log_newest: 0,
        dsp_diag_ok: dsp_ok as u32,
        dsp_stopped: watch::dsp_stopped() as u32,
        dsp_heartbeat: hb,
        crash_found: k.found,
        crash_pc: k.pc,
        crash_vec: k.vec,
        ram_kept: k.ram_kept,
        dsp_crumb: watch::stop_crumb(),
        fault_rows: 0,
        fault_text: [[0; 24]; FAULT_ROWS],
    };
    // the last fault, kept in the settings store (`dk/fault`): CPU crash or DSP stop, the module, the place
    let mut v = [0u8; dkrt_sdk::fault::BYTES];
    if settings::get(dkrt_sdk::fault::KEY, &mut v) == dkrt_sdk::fault::BYTES as i32 {
        if let Some(f) = dkrt_sdk::fault::Fault::decode(&v) {
            let mut rows = [dkrt_sdk::fault::Row::new(), dkrt_sdk::fault::Row::new(), dkrt_sdk::fault::Row::new(), dkrt_sdk::fault::Row::new()];
            let n = dkrt_sdk::fault::rows(&f, &mut rows);
            let mut k = 0;
            while k < n && k < FAULT_ROWS {
                o.fault_text[k] = rows[k].b;
                k += 1;
            }
            o.fault_rows = k as u32;
        }
    }
}

#[cfg(test)]
mod device_import_tests {
    use super::*;
    use dkrt_abi::profiles;

    /// `dk_device_track_count` answers with the device's `tracks`, and 0 for a device file that gives none.
    #[test]
    fn the_track_count_import_reads_the_device_profile() {
        let p = profiles::ALL[0];
        assert_eq!(p.tracks, 16);
        platform::with_profile(p, || assert_eq!(device_track_count(), 16));
        platform::with_profile(dkrt_abi::platform::DeviceProfile { tracks: 0, ..p }, || assert_eq!(device_track_count(), 0));
    }
}
