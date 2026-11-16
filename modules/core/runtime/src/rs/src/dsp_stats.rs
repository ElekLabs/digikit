//! The DSP stats window on the ColdFire side (docs/plans/profiling.md 13.1, P8d).
//!
//! - `dk_dsp_stats_ctl(on, clear)`: the two control words of window 3, written only for the module that owns the span channel.
//! - `dkrt_dsp_stats_feed(tag, a, b)`: `dl_tick` hands every acknowledgement record of the DSP to the mirror (`dkrt_link::stats`).
//! - `dk_dsp_stats_read(out, max)`: the newest complete sweep of the mirror, for the debug module.
use crate::dlog;
use core::ptr::addr_of_mut;
use dkrt_abi::link::{STATS_CLEAR, STATS_ON};
use dkrt_link::stats::{Mirror, WINDOW_WORDS};
use dkrt_sdk::{E_BUSY, E_CONTEXT, OK};

static mut MIRROR: Mirror = Mirror::new();

pub fn mirror() -> &'static mut Mirror {
    unsafe { &mut *addr_of_mut!(MIRROR) }
}

/// `dk_dsp_stats_ctl(on, clear)`, an import of the modules (sdk/sdk.toml). Returns 0, `E_CONTEXT` (no module code is running) or `E_BUSY` (the span
/// channel belongs to another module, or to none).
#[no_mangle]
pub unsafe extern "C" fn dk_dsp_stats_ctl(on: u32, clear: u32) -> i32 {
    let me = dlog::current_source();
    if me == 0 {
        return E_CONTEXT;
    }
    if dlog::span_owner() != me {
        return E_BUSY;
    }
    let l = crate::link::lk();
    l.set_stats_ctl(STATS_ON, on & 1);
    l.set_stats_ctl(STATS_CLEAR, clear);
    OK
}

/// The record of the acknowledgement slot of one block (called by `dl_tick`). Returns 1 for a stats page, 0 for anything else.
#[no_mangle]
pub unsafe extern "C" fn dkrt_dsp_stats_feed(tag: u32, a: u32, b: u32) -> u32 {
    u32::from(mirror().feed(tag, a, b))
}

/// `dk_dsp_stats_read(out, max)`, an import of the modules (sdk/sdk.toml): the newest complete sweep (at most `max` words) and its number, 0 for none.
#[no_mangle]
pub unsafe extern "C" fn dk_dsp_stats_read(out: *mut u32, max: u32) -> u32 {
    let mut w = [0u32; WINDOW_WORDS];
    match mirror().read(&mut w) {
        Some(seq) => {
            let n = (max as usize).min(WINDOW_WORDS);
            if !out.is_null() {
                core::ptr::copy_nonoverlapping(w.as_ptr(), out, n);
            }
            seq
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::LOCK;
    use dkrt_abi::link::Slot;
    use dkrt_link::stats::{tag, PAGES};

    unsafe fn reset() {
        dlog::test_reset_sinks();
        *crate::link::lk() = crate::link::Link::new(crate::link::DspTransport);
        *mirror() = Mirror::new();
    }

    unsafe fn in_module<R>(idx: usize, f: impl FnOnce() -> R) -> R {
        let prev = dlog::enter_module(idx, dlog::PHASE_INIT);
        let r = f();
        dlog::leave_module(prev);
        r
    }

    unsafe extern "C" fn emit(_: *const dlog::Span) {}

    #[test]
    fn only_the_owner_of_the_span_channel_writes_the_control_words() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            reset();
            assert_eq!(dk_dsp_stats_ctl(1, 3), E_CONTEXT, "no module code running");
            assert_eq!(in_module(0, || dk_dsp_stats_ctl(1, 3)), E_BUSY, "nobody owns the channel");
            assert_eq!(in_module(1, || dlog::dk_span_sink_set(emit as usize as *mut u8, 1usize as *mut u8)), OK);
            assert_eq!(in_module(0, || dk_dsp_stats_ctl(1, 3)), E_BUSY, "another module owns it");
            assert!(!crate::link::lk().dirty(Slot::new(3, 0)));
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 3)), OK);
            assert!(crate::link::lk().dirty(Slot::new(3, 0)) && crate::link::lk().dirty(Slot::new(3, 1)));
            reset();
        }
    }

    #[test]
    fn the_mirror_reaches_the_reader_as_whole_sweeps() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            reset();
            let mut out = [0u32; WINDOW_WORDS];
            assert_eq!(dk_dsp_stats_read(out.as_mut_ptr(), 100), 0);
            for p in 0..PAGES as u8 {
                assert_eq!(dkrt_dsp_stats_feed(tag(1, p), p as u32, 7), 1);
            }
            assert_eq!(dkrt_dsp_stats_feed(0xA5D1, 0, 0), 0, "an acknowledgement is not a page");
            assert_eq!(dk_dsp_stats_read(out.as_mut_ptr(), 4), 1);
            assert_eq!(&out[..4], &[0, 7, 1, 7]);
            assert_eq!(out[4], 0, "only `max` words are copied");
            reset();
        }
    }
}
