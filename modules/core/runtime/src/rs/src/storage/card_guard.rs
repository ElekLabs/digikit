//! What the runtime reports of the card write guard (`core/card-guard`, docs/plans/card-write-guard.md): the refusal counters of the hooks in the firmware's eMMC
//! block write and erase. The hooks run in whatever stock task writes the card, so they only count; this file reads the counters (`Platform::card_guard_stats`)
//! for INFO and HEALTH and raises the log event from the UI tick. Nothing here decides anything: the runtime knows the guard by its counters and by the one label
//! that tells its own write from a stock one (`dkrt_raw_write_ret`, tramp.s).
use crate::dlog;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::platform::Platform;
use dkrt_sdk::*;

/// The counters, as `dkcg_stats` of the adapter reports them (all 0 without the module).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// 0: the guard is not in the image. 1: the layout below.
    pub version: u32,
    pub refused_writes: u32,
    pub refused_erases: u32,
    /// The last refused request: first sector, length in sectors, and `class | kind << 8` (class 0 stock, 1 runtime; kind 0 write, 1 erase).
    pub last_sector: u32,
    pub last_sectors: u32,
    pub last_class_kind: u32,
}

impl Stats {
    pub fn words(&self) -> [u32; 6] {
        [self.version, self.refused_writes, self.refused_erases, self.last_sector, self.last_sectors, self.last_class_kind]
    }
    pub fn refused(&self) -> u32 {
        self.refused_writes.wrapping_add(self.refused_erases)
    }
}

pub unsafe fn stats<P: Platform>(p: &P) -> Stats {
    let w = p.card_guard_stats();
    Stats { version: w[0], refused_writes: w[1], refused_erases: w[2], last_sector: w[3], last_sectors: w[4], last_class_kind: w[5] }
}

/// The number of refusals the log has reported, and the clock reading of the last event.
static mut REPORTED: u32 = 0;
static mut LAST_EVENT: u32 = 0;
/// Whether an event was ever raised (the first one needs no wait).
static mut RAISED: bool = false;

/// The least time between two events, in the clock's units (firmware blocks, 1500 per second: 5 seconds). A stock writer that keeps trying a refused request
/// raises one event per window, not one per UI pass.
pub const COOLDOWN: u32 = 7500;

/// Raise one `CARD_GUARD_REFUSED` event when refusals happened since the last event and at least `COOLDOWN` has passed since it (the first event needs no
/// wait): the last request and the totals. Refusals that come during the wait stay counted; the next event after the wait reports them all, so a burst is one
/// event (plus one more for what followed it). `now` is a clock that rises. Returns whether it raised one.
pub unsafe fn tick_with<P: Platform>(p: &P, reported: *mut u32, last: *mut u32, raised: *mut bool, now: u32) -> bool {
    let s = stats(p);
    if s.version == 0 || s.refused() == read_volatile(reported) {
        return false;
    }
    if read_volatile(raised) && now.wrapping_sub(read_volatile(last)) < COOLDOWN {
        return false;
    }
    write_volatile(reported, s.refused());
    write_volatile(last, now);
    write_volatile(raised, true);
    dlog::ev(logk::CARD_GUARD_REFUSED, logk::F_ERROR, [s.last_sector, s.last_sectors, s.last_class_kind, s.refused_writes, s.refused_erases]);
    true
}

/// Called from the UI tick. The clock is the firmware's block counter; without it (no core/health) the UI pass count, which only ever rises too.
pub unsafe fn tick() {
    let now = crate::fw_blocks().unwrap_or_else(|| crate::platform::st().ticks);
    tick_with(&crate::PLATFORM, addr_of_mut!(REPORTED), addr_of_mut!(LAST_EVENT), addr_of_mut!(RAISED), now);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::FakePlatform;
    use dkrt_abi::profiles;

    #[test]
    fn no_guard_in_the_image_reports_zeros_and_raises_nothing() {
        let f = FakePlatform::new(profiles::ALL[0]);
        let mut c = Clock::default();
        unsafe {
            assert_eq!(stats(&f), Stats::default());
            assert!(!c.tick(&f, 0));
        }
    }

    #[test]
    fn the_words_are_the_adapters_in_the_adapters_order() {
        let f = FakePlatform::new(profiles::ALL[0]);
        f.guard_stats.set([1, 3, 2, 0x200000, 8, 0x101]);
        let s = unsafe { stats(&f) };
        assert_eq!((s.version, s.refused_writes, s.refused_erases), (1, 3, 2));
        assert_eq!((s.last_sector, s.last_sectors, s.last_class_kind), (0x200000, 8, 0x101));
        assert_eq!(s.words(), [1, 3, 2, 0x200000, 8, 0x101]);
        assert_eq!(s.refused(), 5);
    }

    #[test]
    fn a_refusal_raises_one_event_with_the_last_request_and_the_totals() {
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let f = FakePlatform::new(profiles::ALL[0]);
        let mut c = Clock::default();
        let events = || unsafe { core::ptr::read_volatile(core::ptr::addr_of!(dlog::DKRT_LOG_COUNT)) };
        let last = || unsafe { (*core::ptr::addr_of!(dlog::DKRT_LOG))[(events() as usize - 1) % dlog::RING] };
        let before = events();
        f.guard_stats.set([1, 0, 0, 0, 0, 0]);
        unsafe {
            assert!(!c.tick(&f, 10), "a guard with no refusal raises nothing");
            f.guard_stats.set([1, 2, 1, 0x200000, 3, 0x100]);
            assert!(c.tick(&f, 20), "the first event needs no wait");
            assert_eq!(c.reported, 3);
            assert!(!c.tick(&f, 30), "the same count again: nothing new");
            assert_eq!(events(), before.wrapping_add(1));
            f.guard_stats.set([1, 2, 2, 0x180000, 1, 0x001]);
            assert!(c.tick(&f, 20 + COOLDOWN), "one event for all refusals since the last one, after the wait");
        }
        assert_eq!(events(), before.wrapping_add(2));
        let e = last();
        assert_eq!(e.kind, logk::CARD_GUARD_REFUSED);
        assert_eq!(e.a, [0x180000, 1, 0x001, 2, 2]);
        assert_eq!(e.flags & logk::F_ERROR, logk::F_ERROR);
    }

    #[test]
    fn a_writer_that_keeps_trying_raises_one_event_per_wait_not_one_per_pass() {
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let f = FakePlatform::new(profiles::ALL[0]);
        let mut c = Clock::default();
        let events = || unsafe { core::ptr::read_volatile(core::ptr::addr_of!(dlog::DKRT_LOG_COUNT)) };
        let before = events();
        let mut raised = 0;
        // 100 000 passes, a new refusal at every one, 5 passes per millisecond-ish: the clock rises by 1 per pass
        for pass in 0..100_000u32 {
            f.guard_stats.set([1, pass + 1, 0, 0x200000, 1, 0]);
            if unsafe { c.tick(&f, 1000 + pass) } {
                raised += 1;
            }
        }
        assert_eq!(raised, 100_000 / COOLDOWN + 1, "one event at the start and one per window");
        assert_eq!(events(), before.wrapping_add(raised));
        // what came during the last wait is reported by the next event, with the totals
        f.guard_stats.set([1, 100_001, 0, 0x200000, 1, 0]);
        assert!(!unsafe { c.tick(&f, 1000 + 100_000) });
        assert!(unsafe { c.tick(&f, 1000 + 100_000 + COOLDOWN) });
    }

    /// The statics of `tick` as a value, for the tests.
    #[derive(Default)]
    struct Clock {
        reported: u32,
        last: u32,
        raised: bool,
    }
    impl Clock {
        unsafe fn tick<P: Platform>(&mut self, p: &P, now: u32) -> bool {
            tick_with(p, &mut self.reported, &mut self.last, &mut self.raised, now)
        }
    }
}
