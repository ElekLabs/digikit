//! The DSP stats window on the ColdFire side (docs/plans/profiling.md 13.1, P8d).
//!
//! - `dk_dsp_stats_ctl(on, clear)`: the control words of window 3 (on, clear generation, the block period of the device), written only for the module that owns the span channel.
//! - `dkrt_dsp_stats_feed(tag, a, b)`: `dl_tick` hands every acknowledgement record of the DSP to the mirror (`dkrt_link::stats`).
//! - `dk_dsp_stats_read(out, max)`: the newest complete sweep of the mirror, for the debug module.
use crate::dlog;
use core::ptr::addr_of_mut;
use dkrt_abi::link::{STATS_CLEAR, STATS_ON, STATS_PERIOD};
use dkrt_link::stats::{Mirror, WINDOW_WORDS};
use dkrt_sdk::{E_BUSY, E_CONTEXT, OK};

extern "C" {
    /// `core/dsp-link`: `void dl_stats_hook(u32 (*cb)(u32, u32, u32))`, the feed of the stats pages from `dl_tick`. 0 when the image has no such function
    /// (`[object] optional`).
    fn dl_stats_hook();
}

/// Words `dk_dsp_stats_read` gives: the window and the three counters.
const READ_WORDS: usize = WINDOW_WORDS + 3;

static mut MIRROR: Mirror = Mirror::new();
/// The clear generation the control words last carried (the published sweep belongs to an older one when it differs).
static mut LAST_CLEAR: u32 = 0;

/// Set (the feed) or clear (0) the hook of `dl_tick`. The feed is set while the counters run and only then: with stats off the slot holds stock data.
unsafe fn set_hook(feed: usize) {
    if core::hint::black_box(dl_stats_hook as usize) == 0 {
        return;
    }
    let set: unsafe extern "C" fn(usize) = core::mem::transmute(dl_stats_hook as unsafe extern "C" fn());
    set(feed);
}

/// The generation the mirror expects is set (a page of another one is ignored).
static mut ARMED: bool = false;

/// The owner of the span channel is gone (its `fini`, a failed `init`, or it gave the channel up): the DSP stops counting and streaming (a stopped DSP writes
/// nothing into the slot, which on the Digitone II holds effect-bus audio) and the feed is unhooked. The mirror keeps its last sweep.
pub unsafe fn owner_gone() {
    set_hook(0);
    crate::link::lk().set_stats_ctl(STATS_ON, 0);
}

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
    if !ARMED || clear != LAST_CLEAR {
        // the DSP clears its window for the new generation: the sweep the mirror holds, and what it counted, are stale. The block interrupt feeds the mirror:
        // it is masked while the main context starts it again.
        let sr = crate::irq_mask();
        LAST_CLEAR = clear;
        ARMED = true;
        mirror().reset_for(clear as u8);
        crate::irq_restore(sr);
    }
    set_hook(if on & 1 != 0 { dkrt_dsp_stats_feed as usize } else { 0 });
    let l = crate::link::lk();
    l.set_stats_ctl(STATS_ON, on & 1);
    l.set_stats_ctl(STATS_CLEAR, clear);
    l.set_stats_ctl(STATS_PERIOD, crate::profile().dsp_block_cycles);
    OK
}

/// The record of the acknowledgement slot of one block (called by `dl_tick`). Returns 1 for a stats page, 0 for anything else.
#[no_mangle]
pub unsafe extern "C" fn dkrt_dsp_stats_feed(tag: u32, a: u32, b: u32) -> u32 {
    let m = mirror();
    let before = m.counts.sweeps;
    let took = m.feed(tag, a, b);
    if m.counts.sweeps != before {
        publish_event(m);
    }
    u32::from(took)
}

/// A complete sweep was published (once per sweep, about every 50 blocks): the `DSP_SWEEP` event, so a host waits on it instead of polling PROFILE_DSP. `ev` masks
/// interrupts itself, so this is safe from the block interrupt; the source is the owner of the span channel (the debug module), whose level filter applies.
unsafe fn publish_event(m: &Mirror) {
    let owner = dlog::span_owner();
    // no owner: `ev` would take the source of the module the interrupt hit, so there is no event (the counters are about to stop with the owner anyway)
    if owner == 0 {
        return;
    }
    if let Some((sweep, generation, blocks)) = m.published() {
        let src = (owner as u16) << dkrt_sdk::logk::SOURCE_SHIFT;
        dlog::ev(dkrt_sdk::logk::DSP_SWEEP, dlog::INFO | src, [sweep, generation as u32, blocks, 0, 0]);
    }
}

/// `dk_dsp_stats_read(out, max)`, an import of the modules (sdk/sdk.toml): the newest complete sweep and its number, 0 for none. `out` gets at most `max`
/// words of the 103 the call can give: the 100 words of the window (0 when there is no sweep), then the mirror's counters (sweeps published, sweeps ended
/// by a gap, records ignored), which are valid with or without a sweep.
#[no_mangle]
pub unsafe extern "C" fn dk_dsp_stats_read(out: *mut u32, max: u32) -> u32 {
    let mut w = [0u32; READ_WORDS];
    let m = mirror();
    let seq = m.read(array_mut_ref(&mut w)).unwrap_or(0);
    w[WINDOW_WORDS] = m.counts.sweeps;
    w[WINDOW_WORDS + 1] = m.counts.gaps;
    w[WINDOW_WORDS + 2] = m.counts.ignored;
    let n = (max as usize).min(READ_WORDS);
    if !out.is_null() {
        core::ptr::copy_nonoverlapping(w.as_ptr(), out, n);
    }
    seq
}

fn array_mut_ref(w: &mut [u32; READ_WORDS]) -> &mut [u32; WINDOW_WORDS] {
    (&mut w[..WINDOW_WORDS]).try_into().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::LOCK;
    use dkrt_abi::link::Slot;
    use dkrt_link::stats::{tag, PAGES};

    unsafe fn reset() {
        LAST_CLEAR = 0;
        ARMED = false;
        crate::cleanup_tests::STATS_HOOK.store(0, core::sync::atomic::Ordering::SeqCst);
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
            assert!(
                crate::link::lk().dirty(Slot::new(3, 0))
                    && crate::link::lk().dirty(Slot::new(3, 1))
                    && crate::link::lk().dirty(Slot::new(3, 2))
            );
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
            let mut all = [0u32; 103];
            assert_eq!(dk_dsp_stats_read(all.as_mut_ptr(), 103), 1);
            assert_eq!(&all[100..], &[1, 0, 1], "sweeps, gaps and ignored records follow the window");
            reset();
        }
    }

    #[test]
    fn a_new_clear_generation_drops_the_published_sweep_and_the_counts() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            reset();
            in_module(1, || dlog::dk_span_sink_set(emit as usize as *mut u8, 1usize as *mut u8));
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 0)), OK, "the first start arms the mirror for generation 0");
            for p in 0..PAGES as u8 {
                dkrt_dsp_stats_feed(tag(0, p), p as u32, 7);
            }
            let mut out = [0u32; 103];
            assert_eq!(dk_dsp_stats_read(out.as_mut_ptr(), 103), 1);
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 0)), OK, "the same generation keeps the sweep");
            assert_eq!(dk_dsp_stats_read(out.as_mut_ptr(), 103), 1);
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 1)), OK);
            assert_eq!(dk_dsp_stats_read(out.as_mut_ptr(), 103), 0, "a new generation: no sweep until the DSP sends one");
            assert_eq!(&out[100..], &[0, 0, 0]);
            reset();
        }
    }

    fn hook() -> usize {
        crate::cleanup_tests::STATS_HOOK.load(core::sync::atomic::Ordering::SeqCst)
    }

    #[test]
    fn the_feed_is_hooked_only_while_the_counters_run_and_a_page_of_another_generation_is_ignored() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            reset();
            in_module(1, || dlog::dk_span_sink_set(emit as usize as *mut u8, 1usize as *mut u8));
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 5)), OK);
            assert_ne!(hook(), 0, "on: dl_tick feeds the mirror");
            assert_eq!(dkrt_dsp_stats_feed(tag(5, 0), 1, 2), 1);
            assert_eq!(dkrt_dsp_stats_feed(tag(6, 1), 1, 2), 0, "another generation: stock data that carries the magic");
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(0, 5)), OK);
            assert_eq!(hook(), 0, "off: the slot holds stock data and dl_tick does not look at it");
            reset();
        }
    }

    #[test]
    fn the_dsp_counters_stop_when_the_owner_of_the_span_channel_is_released() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            reset();
            in_module(1, || dlog::dk_span_sink_set(emit as usize as *mut u8, 1usize as *mut u8));
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 0)), OK);
            assert_ne!(hook(), 0);
            dlog::release_sinks(1);
            assert_eq!(hook(), 0, "a module that is gone without a fini: no feed");
            assert_eq!(dlog::span_owner(), 0);
            reset();
        }
    }

    static mut SEEN: [(u16, u16, [u32; 5]); 8] = [(0, 0, [0; 5]); 8];
    static mut SEEN_N: usize = 0;

    unsafe extern "C" fn log_sink(e: *const dlog::Ev) {
        // only this kind: the replay of the small ring to a new sink brings the other events
        if (*e).kind == dkrt_sdk::logk::DSP_SWEEP && SEEN_N < 8 {
            SEEN[SEEN_N] = ((*e).kind, (*e).flags, (*e).a);
            SEEN_N += 1;
        }
    }

    #[test]
    fn a_complete_sweep_logs_one_dsp_sweep_event_with_its_number_generation_and_blocks() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            reset();
            in_module(1, || dlog::dk_span_sink_set(emit as usize as *mut u8, 1usize as *mut u8));
            dlog::dk_debug_log_sink_set(log_sink as usize as *mut u8, core::ptr::null_mut());
            SEEN_N = 0; // the replay of the small ring (kept before the first UI frame) brought events of earlier tests
            assert_eq!(in_module(1, || dk_dsp_stats_ctl(1, 4)), OK);
            let events = |kind| (0..SEEN_N).filter(|&i| SEEN[i].0 == kind).count();
            for p in 0..PAGES as u8 - 1 {
                dkrt_dsp_stats_feed(tag(4, p), 1234, 7);
            }
            assert_eq!(events(dkrt_sdk::logk::DSP_SWEEP), 0, "no event before the sweep is complete");
            dkrt_dsp_stats_feed(tag(4, PAGES as u8 - 1), 1, 7);
            assert_eq!(events(dkrt_sdk::logk::DSP_SWEEP), 1, "one event when it completes");
            let at = (0..SEEN_N).find(|&i| SEEN[i].0 == dkrt_sdk::logk::DSP_SWEEP).unwrap();
            let (_, flags, a) = SEEN[at];
            assert_eq!(&a[..3], &[1, 4, 1234], "sweep number, generation and the blocks of the header");
            assert_eq!(flags >> dkrt_sdk::logk::SOURCE_SHIFT, 2, "the source is the owner of the span channel (record 1 is source 2)");
            // a repeated page or a page of the next sweep that is not complete adds none
            dkrt_dsp_stats_feed(tag(4, 0), 1, 7);
            assert_eq!(events(dkrt_sdk::logk::DSP_SWEEP), 1);
            dlog::dk_debug_log_sink_set(core::ptr::null_mut(), core::ptr::null_mut());
            reset();
        }
    }
}
