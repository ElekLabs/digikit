//! Safe mode and the stable period: the safe-mode key, and the UI-loop pass that clears the boot counter once the session has been stable
//! (`DKRT_STABLE_BLOCKS` DSP blocks, in a safe boot too). A core-only boot never clears it (`KEEP_COUNTER`).

use crate::boot::{info, INFO_MAGIC};
use crate::dlog;
use crate::card;
use crate::level::Level;
use crate::selection::{current_with, update_with, Choice};
use crate::platform::PLATFORM;
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
/// This boot is core-only, whatever the cause (`Level::keeps_counter`): the stable period does not clear the counter. Writing the system package file or RETRY clears it (`clear_counter_with`).
pub(crate) static mut KEEP_COUNTER: bool = false;
/// UI-loop passes after which a block counter that has not moved at all is taken for dead and the counter is cleared as in the first
/// pass of old (the UI loop runs 30 to 120 times a second: 1800 passes are 15 s to a minute).
const DEAD_CLOCK_PASSES: u32 = 1800;

/// Is the safe-mode key down? Reads the stock key state byte. REC is key 0x13, panel channel 2 bit 2. Returns 0 before the first
/// channel-2 report. Hardware timing not yet proved **[O]**.
pub(crate) unsafe fn key_held() -> bool {
    key_held_with(&crate::platform::PLATFORM)
}

/// `key_held` over a platform (the host tests drive it with `FakePlatform`).
pub(crate) unsafe fn key_held_with<P: Platform>(p: &P) -> bool {
    match p.safe_key() {
        Some(k) => p.key_down(k),
        None => false, // the device has no key that starts safe mode
    }
}

/// Every pass of the UI loop (`dkrt_tick`). The first pass notes the block count; the boot counter goes back to 0 when the session has run
/// `DKRT_STABLE_BLOCKS` blocks past it (a safe boot too: the counter of a safe boot the counter caused stays until then). A hang or a fault in between
/// leaves the counter raised, so the next boots count it: two such boots in a row start the third in safe mode, four the next one core-only.
pub unsafe fn ui_pass() {
    ui_pass_with(&PLATFORM)
}

/// `ui_pass` over a platform (the host tests drive it with `FakePlatform`, whose block counter they move).
pub(crate) unsafe fn ui_pass_with<P: Platform>(p: &P) {
    let b = info();
    if b.stable != 0 {
        return;
    }
    if b.magic != INFO_MAGIC {
        b.ui_reached = 1;
        b.stable = 1;
        return; // the boot hook did not run (an image patched without the boot site)
    }
    let now = p.blocks().unwrap_or(0);
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
    if elapsed >= period || dead {
        b.stable = 1;
        // A boot that followed a crash in a stable session leaves one bad boot counted (`LATE_FLOOR`), so that a second such crash leads to safe mode.
        // With SAFE_READ the record is unknown: the counter stays as it is. A core-only boot keeps it (`KEEP_COUNTER`).
        let floor = LATE_FLOOR;
        if !KEEP_COUNTER {
            clear_counter_with(p, floor, b.reason == SAFE_READ);
        }
        dlog::ev(logk::STABLE, dlog::INFO, [now, floor, 0, 0, 0]);
    }
}

/// Do the boots counted in the record call for safe mode (`SAFE_AFTER` boots that did not reach the stable period)?
pub(crate) fn counter_is_safe(r: &Record) -> bool {
    crate::level::counter_is_safe(r.boot_counter)
}

/// Count this boot: raise the boot counter in the selection record and write it before anything of a module runs. Every normal boot counts, whether
/// or not a selection exists: a fresh device has no record, and the write creates it (`update` with no valid copy: copy A, `seq` 1). Of the other
/// levels only a boot the counter or crashes caused counts itself (`Level::counts_boot`): a key or one-shot-flag safe boot does not.
/// On a card the runtime may not write (a foreign area, unreadable, too small, no store, or not checked yet and then refused) nothing is written and
/// nothing counts. `writable` is `card::writable()` as the caller read it after the card check. Returns the record this boot goes on with.
pub(crate) unsafe fn count_boot_with<P: Platform>(p: &P, level: Level, r: Record, copy: u32, valid: bool, writable: bool) -> Record {
    if !writable || !level.counts_boot() {
        return r;
    }
    let mut n = r;
    n.boot_counter = n.boot_counter.saturating_add(1);
    update_with(p, n, copy, valid);
    n
}

/// The counter of a boot that found the crash record of a session that had been stable: that boot had cleared its counter, so it is counted bad now.
/// Returns the floor the next stable period leaves (`LATE_FLOOR`).
pub(crate) fn count_late_crash(r: &mut Record) -> u32 {
    r.boot_counter = r.boot_counter.saturating_add(1);
    1
}

/// The system package file is being written (the store reports it before its commit writes, so that a reset between the two cannot leave the new package with
/// the counter at `CORE_ONLY_AFTER` or more): the counter goes back to 0, and the running boot's stable period leaves 0 too (a new package starts with no bad boot counted, also the
/// one a crash record of the boot before set: `LATE_FLOOR` goes). The running boot is not counted any more either: its own early crash leaves 0.
pub(crate) unsafe fn package_written_with<P: Platform>(p: &P) {
    LATE_FLOOR = 0;
    clear_counter_with(p, 0, false);
}

/// The stable period is over, the package file was written or RETRY ran: the counter in the record as the card holds it now goes back to `floor`. `unknown`: the record
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
mod tests;
