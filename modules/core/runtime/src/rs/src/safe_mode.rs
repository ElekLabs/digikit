//! Safe mode and the stable period: the safe-mode key, and the UI-loop pass that clears the boot counter once the session has been stable
//! (`DKRT_STABLE_BLOCKS` DSP blocks, or at once in a safe boot).

use crate::boot::{info, INFO_MAGIC};
use crate::dlog;
use crate::card;
use crate::selection::{current_with, update_with, Choice};
use crate::PLATFORM;
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
        // A boot that followed a crash in a stable session leaves one bad boot counted (`LATE_FLOOR`), so that a second such crash leads to safe mode.
        // With SAFE_READ the record is unknown: the counter stays as it is.
        let floor = LATE_FLOOR;
        clear_counter_with(&PLATFORM, floor, b.reason == SAFE_READ);
        dlog::ev(logk::STABLE, dlog::INFO, [now, floor, 0, 0, 0]);
    }
}

/// Do the boots counted in the record call for safe mode (`SAFE_AFTER` boots that did not reach the stable period)?
pub(crate) fn counter_is_safe(r: &Record) -> bool {
    r.boot_counter >= SAFE_AFTER
}

/// Count this boot: raise the boot counter in the selection record and write it before anything of a module runs. Every boot that is not safe
/// counts, whether or not a selection exists: a fresh device has no record, and the write creates it (`update` with no valid copy: copy A, `seq` 1).
/// On a card the runtime may not write (a foreign area, unreadable, too small, no store, or not checked yet and then refused) nothing is written and
/// nothing counts. `writable` is `card::writable()` as the caller read it after the card check. Returns the record this boot goes on with.
pub(crate) unsafe fn count_boot_with<P: Platform>(p: &P, r: Record, copy: u32, valid: bool, writable: bool) -> Record {
    if !writable {
        return r;
    }
    let mut n = r;
    n.boot_counter = n.boot_counter.wrapping_add(1);
    update_with(p, n, copy, valid);
    n
}

/// The counter of a boot that found the crash record of a session that had been stable: that boot had cleared its counter, so it is counted bad now.
/// Returns the floor the next stable period leaves (`LATE_FLOOR`).
pub(crate) fn count_late_crash(r: &mut Record) -> u32 {
    r.boot_counter = r.boot_counter.wrapping_add(1);
    1
}

/// The stable period is over (or the boot is safe): the counter in the record as the card holds it now goes back to `floor`. `unknown`: the record
/// could not be read at the boot, the counter stays as it is.
pub(crate) unsafe fn clear_counter_with<P: Platform>(p: &P, floor: u32, unknown: bool) {
    // the record as the card holds it now, not the one this boot started from: the menu may have saved a selection since
    let cur = match current_with(p) {
        Choice::Use { record, copy, .. } if !unknown => Some((record, copy)),
        _ => None,
    };
    if let Some((mut r, copy)) = cur {
        if r.boot_counter != floor {
            r.boot_counter = floor;
            update_with(p, r, copy, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::FakePlatform;
    use crate::storage::layout::TEST_LAYOUT;
    use crate::test_lock::LOCK;
    use dkrt_abi::profiles;
    use dkrt_sdk::store::Layout;

    const L: Layout = Layout { first: 1000, sectors: 4096 };
    const FOREIGN: [u8; 512] = [0x55; 512];

    /// A new boot: the runtime's RAM (the card state) starts again, the card stays.
    fn new_boot() {
        *card::info() = card::CardInfo {
            state: CARD_UNKNOWN,
            first_nonblank: u32::MAX,
            hash: 0,
            refused: 0,
            refused_range: 0,
            read_errors: 0,
            checked: 0,
            claims: 0,
            writes: 0,
            sectors: 0,
        };
        TEST_LAYOUT.with(|c| c.set(Some(L)));
    }

    /// How a boot ends.
    #[derive(Clone, Copy)]
    enum End {
        /// The session stops before the stable period (a hang, a power-off, a crash: the counter was raised at the start already).
        Early,
        /// The stable period passes.
        Stable,
    }

    /// One boot over the runtime's own functions, in the order of `dkrt_boot` and `ui_pass`. `late_crash`: the boot finds the crash record of a
    /// session that had been stable. Returns whether the boot was safe mode by counter.
    unsafe fn boot(f: &FakePlatform, late_crash: bool, end: End) -> bool {
        new_boot();
        let (mut r, copy, valid) = match current_with(f) {
            Choice::Use { record, copy, .. } => (record, copy, true),
            _ => (Record::empty(), 0, false),
        };
        let floor = if late_crash { count_late_crash(&mut r) } else { 0 };
        if counter_is_safe(&r) {
            // a safe boot loads nothing and counts nothing; its first UI-loop pass clears the counter
            clear_counter_with(f, 0, false);
            return true;
        }
        count_boot_with(f, r, copy, valid, card::writable());
        if let End::Stable = end {
            clear_counter_with(f, floor, false);
        }
        false
    }

    /// The record the card holds now.
    unsafe fn held(f: &FakePlatform) -> Option<Record> {
        new_boot();
        match current_with(f) {
            Choice::Use { record, .. } => Some(record),
            _ => None,
        }
    }

    fn fresh_card() -> FakePlatform {
        FakePlatform::new(profiles::ALL[0])
    }

    fn written_sectors(f: &FakePlatform) -> Vec<u32> {
        let mut v: Vec<u32> = f.writes.borrow().iter().map(|w| w.0).collect();
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn a_fresh_device_that_does_not_reach_the_stable_period_boots_safe_on_the_third_boot() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let f = fresh_card();
            assert!(held(&f).is_none(), "a fresh card has no record");
            assert!(!boot(&f, false, End::Early));
            let r = held(&f).expect("the first counted boot created the record");
            assert_eq!((r.seq, r.boot_counter, r.count), (1, 1, 0));
            assert_eq!(written_sectors(&f), [L.sel_a()], "only a selection copy was written");
            assert!(!boot(&f, false, End::Early));
            assert_eq!(held(&f).unwrap().boot_counter, 2);
            assert!(boot(&f, false, End::Early), "two bad boots: the third is safe");
            let r = held(&f).unwrap();
            assert_eq!(r.boot_counter, 0, "the safe boot cleared the counter");
            assert_eq!(r.count, 0);
            assert!(!boot(&f, false, End::Stable), "the boot after it tries the selection again");
        }
    }

    #[test]
    fn a_stable_session_clears_the_counter_so_later_failures_count_from_zero() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let f = fresh_card();
            let seen: Vec<bool> = [End::Early, End::Stable, End::Early, End::Early, End::Early]
                .into_iter()
                .map(|end| boot(&f, false, end))
                .collect();
            assert_eq!(seen, [false, false, false, false, true]);
        }
    }

    #[test]
    fn a_crash_after_the_stable_period_counts_once_and_two_in_a_row_lead_to_safe_mode() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let f = fresh_card();
            assert!(!boot(&f, false, End::Stable), "a stable first session");
            assert_eq!(held(&f).unwrap().boot_counter, 0);
            // the session crashed after its stable period: the next boot counts it (1) and itself (2), and its own stable period leaves 1
            assert!(!boot(&f, true, End::Stable));
            assert_eq!(held(&f).unwrap().boot_counter, 1);
            assert!(boot(&f, true, End::Stable), "a second such crash: the boot is safe");
        }
    }

    #[test]
    fn a_card_the_runtime_may_not_write_counts_nothing_and_never_loops_into_safe_mode() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let foreign = fresh_card();
            foreign.put(L.first + 100, FOREIGN);
            let small = fresh_card();
            small.card_sectors.set(L.first + 10); // the card ends inside the store area: no store
            for (name, f) in [("foreign bytes in the area", &foreign), ("a card too small for the store", &small)] {
                for n in 0..6 {
                    assert!(!boot(f, false, End::Early), "{name}: boot {n}");
                }
                assert!(f.writes.borrow().is_empty(), "{name}: nothing was written");
                assert!(held(f).is_none(), "{name}: no record");
            }
        }
    }

    #[test]
    fn an_existing_selection_keeps_its_paths_and_counts_into_the_copy_that_is_not_current() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let f = fresh_card();
            let mut r = Record::empty();
            r.seq = 5;
            r.count = 1;
            r.paths[0][..5].copy_from_slice(b"a.dkm");
            let mut b = [0u8; 512];
            dkrt_sdk::sel::encode(&r, &mut b);
            f.put(L.sel_a(), b);
            assert!(!boot(&f, false, End::Early));
            let now = held(&f).unwrap();
            assert_eq!((now.seq, now.boot_counter, now.count), (6, 1, 1));
            assert_eq!(&now.paths[0][..5], b"a.dkm");
            assert_eq!(written_sectors(&f), [L.sel_b()], "copy A stays valid while B is written");
        }
    }

    #[test]
    fn a_record_that_could_not_be_read_keeps_its_counter_at_the_stable_period() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let f = fresh_card();
            assert!(!boot(&f, false, End::Early));
            new_boot();
            clear_counter_with(&f, 0, true);
            assert_eq!(held(&f).unwrap().boot_counter, 1);
        }
    }
}
