//! Safe mode and the stable period: the safe-mode key, and the UI-loop pass that clears the boot counter once the session has been stable
//! (`DKRT_STABLE_BLOCKS` DSP blocks, or at once in a safe boot).

use crate::boot::{info, INFO_MAGIC};
use crate::dlog;
use crate::selection::{current, update, Choice};
use core::ptr::addr_of;
use dkrt_abi::platform::Platform;
use dkrt_sdk::sel::Record;
use dkrt_sdk::*;

/// The stable period in DSP blocks (`STABLE_BLOCKS`). A static so that the scenarios can shorten it (they poke it; nothing else writes it).
#[repr(C, align(4))]
pub struct Knob(pub u32); // a ColdFire gcc aligns a u32 to 2: the emulator's poke wants a word
#[no_mangle]
pub static mut DKRT_STABLE_BLOCKS: Knob = Knob(STABLE_BLOCKS);
/// The block count at the first UI-loop pass and the number of passes since (the fallback when the block counter does not move).
#[no_mangle]
pub static mut DKRT_UI_START: Knob = Knob(0);
static mut UI_PASSES: u32 = 0;
/// The boot counter a stable session leaves behind: 1 after a boot that found the crash record of a session that had been stable, else 0.
pub(crate) static mut LATE_FLOOR: u32 = 0;
/// UI-loop passes after which a block counter that has not moved at all is taken for dead and the counter is cleared as in the first
/// pass of old (the UI loop runs 30 to 120 times a second: 1800 passes are 15 s to a minute).
const DEAD_CLOCK_PASSES: u32 = 1800;

/// Is the safe-mode key down? Reads the stock key state byte. REC is key 0x13, panel channel 2 bit 2. Returns 0 before the first
/// channel-2 report. Hardware timing not yet proved **[O]**.
pub(crate) unsafe fn key_held() -> bool {
    match crate::PLATFORM.safe_key() {
        Some(k) => crate::PLATFORM.key_down(k),
        None => false, // the device has no key that starts safe mode
    }
}

/// Every pass of the UI loop (`dkrt_tick`). The first pass notes the block count; the boot counter goes back to 0 when the session has run
/// `DKRT_STABLE_BLOCKS` blocks past it, or at once in a safe boot (nothing of a module runs there). A hang or a fault in between leaves
/// the counter raised, so the next boots count it: two such boots in a row start the third in safe mode.
pub unsafe fn ui_pass() {
    let b = info();
    if b.stable != 0 {
        return;
    }
    if b.magic != INFO_MAGIC {
        b.ui_reached = 1;
        b.stable = 1;
        return; // the boot hook did not run (an image patched without the boot site)
    }
    let now = crate::fw_blocks().unwrap_or(0);
    if b.ui_reached == 0 {
        b.ui_reached = 1;
        DKRT_UI_START.0 = now;
        UI_PASSES = 0;
        dlog::ev(logk::UI_FIRST, dlog::INFO, [now, b.safe, b.loaded, 0, 0]);
        // The UI loop starts only after the boot intro: a host that waits for FW_READY may use the stock MidiRpc service from here on.
        dlog::ev(logk::FW_READY, dlog::INFO, [now, 0, 0, 0, 0]);
    }
    UI_PASSES = UI_PASSES.wrapping_add(1);
    let elapsed = now.wrapping_sub(DKRT_UI_START.0);
    let period = core::ptr::read_volatile(addr_of!(DKRT_STABLE_BLOCKS.0));
    let dead = elapsed == 0 && UI_PASSES >= DEAD_CLOCK_PASSES;
    if b.safe != 0 || elapsed >= period || dead {
        b.stable = 1;
        // The record as the card holds it now, not the one this boot started from: the menu may have saved a selection since.
        // A boot that followed a crash in a stable session leaves one bad boot counted (`LATE_FLOOR`), so that a second such crash leads to safe mode.
        let floor = LATE_FLOOR;
        // With SAFE_READ the record is unknown: the counter stays as it is.
        let cur = match current() {
            Choice::Use { record, copy, .. } if b.reason != SAFE_READ => Some((record, copy)),
            _ => None,
        };
        if let Some((mut r, copy)) = cur {
            if r.boot_counter != floor {
                r.boot_counter = floor;
                update(r, copy, true);
            }
        }
        dlog::ev(logk::STABLE, dlog::INFO, [now, floor, 0, 0, 0]);
    }
}

/// The record with this boot counted, or `None` when the card is not one the runtime may write (no counter then). Every boot that is not
/// safe counts, whether or not a selection exists: a fresh device has none, and the write creates the record (`update` with no valid copy).
pub(crate) fn raised(r: &Record, writable: bool) -> Option<Record> {
    if !writable {
        return None;
    }
    let mut n = *r;
    n.boot_counter = n.boot_counter.wrapping_add(1);
    Some(n)
}

/// The counter after a boot that found the crash record of a session that had been stable: that boot had cleared its counter, so it is counted now.
pub(crate) fn after_late_crash(counter: u32) -> u32 {
    counter.wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the card holds and what the next boot reads: `None` is a fresh device (`Choice::Empty`).
    struct Card {
        rec: Option<Record>,
        writable: bool,
    }

    /// How a boot ends.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum End {
        /// Hangs or loses power before the stable period.
        Hang,
        /// A CPU crash before the stable period (the crash record is found at the next boot; its counter was not cleared, so no extra count).
        Crash,
        /// Runs through the stable period.
        Stable,
    }

    /// One boot over the same decisions as `dkrt_boot` and `ui_pass`; returns whether it was a safe boot.
    fn boot(c: &mut Card, end: End) -> bool {
        let mut r = c.rec.unwrap_or_else(Record::empty);
        if r.boot_counter >= SAFE_AFTER {
            // a safe boot loads nothing and counts nothing; its UI loop clears the counter at once
            if c.writable {
                r.boot_counter = 0;
                c.rec = Some(r);
            }
            return true;
        }
        if let Some(n) = raised(&r, c.writable) {
            c.rec = Some(n);
        }
        if end == End::Stable && c.writable {
            let mut r = c.rec.unwrap();
            r.boot_counter = 0;
            c.rec = Some(r);
        }
        false
    }

    #[test]
    fn a_fresh_device_that_does_not_reach_the_stable_period_boots_safe_on_the_third_boot() {
        for end in [End::Hang, End::Crash] {
            let mut c = Card { rec: None, writable: true };
            let safe: Vec<bool> = (0..3).map(|_| boot(&mut c, end)).collect();
            assert_eq!(safe, [false, false, true], "{end:?}");
            // the safe boot cleared the counter: the next boot tries the selection again
            assert_eq!(c.rec.unwrap().boot_counter, 0);
            assert!(!boot(&mut c, End::Stable));
        }
    }

    #[test]
    fn a_stable_session_clears_the_counter_so_later_hangs_start_from_zero() {
        let mut c = Card { rec: None, writable: true };
        assert!(!boot(&mut c, End::Hang));
        assert_eq!(c.rec.unwrap().boot_counter, 1);
        assert!(!boot(&mut c, End::Stable));
        assert_eq!(c.rec.unwrap().boot_counter, 0);
        assert!(!boot(&mut c, End::Hang));
        assert!(!boot(&mut c, End::Hang));
        assert!(boot(&mut c, End::Hang));
    }

    #[test]
    fn a_crash_after_the_stable_period_counts_one_bad_boot_once_the_next_boot_finds_it() {
        // the crashed boot had cleared its counter (0): the next boot counts it (1) and its own start (2): two bad boots lead to safe mode
        let after = after_late_crash(0);
        assert_eq!(after, 1);
        let r = Record::empty();
        let mut r = r;
        r.boot_counter = after;
        assert_eq!(raised(&r, true).unwrap().boot_counter, 2);
    }

    #[test]
    fn a_card_the_runtime_does_not_write_has_no_counter_and_no_safe_mode_by_it() {
        // read-only, too small, or without a store: `writable()` is false; nothing is written and a hang loop never turns safe
        let mut c = Card { rec: None, writable: false };
        assert!(raised(&Record::empty(), false).is_none());
        for _ in 0..10 {
            assert!(!boot(&mut c, End::Hang));
        }
        assert!(c.rec.is_none());
    }

    #[test]
    fn an_existing_record_keeps_its_selection_and_counts_the_same_way() {
        let mut r = Record::empty();
        r.count = 2;
        r.seq = 5;
        let n = raised(&r, true).unwrap();
        assert_eq!((n.count, n.seq, n.boot_counter), (2, 5, 1));
    }
}
