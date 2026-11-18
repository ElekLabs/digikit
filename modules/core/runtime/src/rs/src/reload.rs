//! Reload of a content module (docs/plans/extension-model.md E5): what the runtime holds once for every kind of module. This file has the refusal order;
//! the job, the loader steps and the DSP wait follow in the other files of E5. Per-kind work sits behind the `Service` trait, never in here.

use dkrt_abi::platform::{blocks_since, Platform};
use dkrt_sdk::*;

/// DSP blocks the reload waits after it closed every way into the module's code (its tick, its commands, its sinks): the tasks that were inside have
/// returned and a DSP block that read its rows has ended. The block clock runs about 1500 times a second.
pub const DRAIN_BLOCKS: u32 = 2;

/// Have `DRAIN_BLOCKS` blocks passed since `start` (the block count when the entries were closed)? A platform with no block clock (a device without a DSP,
/// the host tests) has nothing to wait for: always true.
pub fn drained<P: Platform>(p: &P, start: Option<u32>) -> bool {
    blocks_since(p, start).is_none_or(|n| n >= DRAIN_BLOCKS)
}

/// 1 while a reload runs: from the check of the new file until its DSP rows are in place (`CoreApi::reload_phase`). The reload job sets and clears it; a service
/// reads it in `on_loaded`.
static mut PHASE: u32 = 0;

/// The reload phase now.
pub fn phase() -> u32 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PHASE)) }
}

/// Start (`on`) or end the reload phase. Only the reload job calls it.
#[allow(dead_code)] // the reload job calls it (E5 commit 8)
pub(crate) fn set_phase(on: bool) {
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PHASE), on as u32) }
}

/// A reload finished: the counter that tells a host and the menus their lists of modules and machines are out of date (`DkrtBootInfo.reloads`).
#[allow(dead_code)] // the reload job calls it (E5 commit 8)
pub(crate) unsafe fn note_reloaded() {
    let b = crate::boot::info();
    b.reloads = b.reloads.wrapping_add(1);
}

/// What the caller knows about the system when a reload is asked for. Each field is read where it lives (the loader, the card, the module's
/// records); `refusal` only puts the answers in order.
#[derive(Clone, Copy, Default)]
pub struct Facts {
    /// This boot is in safe mode.
    pub safe_mode: bool,
    /// A card transfer or a store job is in flight.
    pub card_busy: bool,
    /// The module built menus of its own (its header names a menu entry): they keep pointers into its code.
    pub menus: bool,
}

/// The refusal of a reload, or `None` when it may go on. One function, one order: safe mode, the firmware's transport (playing, paused, or not
/// readable), card operations, menus of the module. The first that holds names the status. The services' own refusals (`Service::reload_refusal`, for
/// example an effect on a route) come after it: `refusal_with_services`.
pub unsafe fn refusal<P: Platform>(p: &P, f: &Facts) -> Option<i32> {
    if f.safe_mode {
        return Some(E_SAFE_MODE);
    }
    match p.play_state() {
        Some(0) => {}
        _ => return Some(E_RELOAD_PLAYING),
    }
    if f.card_busy {
        return Some(E_CARD_BUSY);
    }
    if f.menus {
        return Some(E_RELOAD_MENUS);
    }
    None
}

/// `refusal`, then the refusals of the services about module record `owner - 1` (`Service::reload_refusal`, every service in table order). The reload job calls this.
pub unsafe fn refusal_with_services<P: Platform>(p: &P, f: &Facts, services: &dkrt_abi::service::ServiceTable, owner: u8) -> Option<i32> {
    if let Some(r) = refusal(p, f) {
        return Some(r);
    }
    let r = services.reload_refusal(owner);
    if r != OK {
        return Some(r);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::FakePlatform;

    fn code(p: &FakePlatform, f: Facts) -> Option<i32> {
        unsafe { refusal(p, &f) }
    }
    fn all() -> Facts {
        Facts {
            safe_mode: true,
            card_busy: true,
            menus: true,
        }
    }

    #[test]
    fn a_stopped_sequencer_and_no_other_fact_allows_the_reload() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        assert_eq!(code(&p, Facts::default()), None);
    }

    #[test]
    fn playing_paused_mixed_or_unreadable_transport_refuses_the_reload() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        for s in [Some(1), Some(2), Some(3), None] {
            p.play.set(s);
            assert_eq!(code(&p, Facts::default()), Some(E_RELOAD_PLAYING), "{s:?}");
        }
    }

    #[test]
    fn the_refusals_come_in_the_documented_order() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        p.play.set(Some(1));
        let mut f = all();
        assert_eq!(code(&p, f), Some(E_SAFE_MODE));
        f.safe_mode = false;
        assert_eq!(code(&p, f), Some(E_RELOAD_PLAYING));
        p.play.set(Some(0));
        assert_eq!(code(&p, f), Some(E_CARD_BUSY));
        f.card_busy = false;
        assert_eq!(code(&p, f), Some(E_RELOAD_MENUS));
        f.menus = false;
        assert_eq!(code(&p, f), None);
    }

    use crate::core_api::{CoreApi, REAL_CORE};
    use dkrt_abi::service::{RowPtrs, Service, ServiceRow, ServiceTable};

    /// A service that refuses the reload of module owner 2 only.
    struct Refuser;
    impl<C: CoreApi> Service<C> for Refuser {
        dkrt_abi::accepts_records!();
        unsafe fn reload_refusal(_core: &C, owner: u8) -> i32 {
            if owner == 2 {
                E_RELOAD_ROUTED
            } else {
                OK
            }
        }
    }
    struct Quiet;
    impl<C: CoreApi> Service<C> for Quiet {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_reload!();
    }
    static ROWS: [ServiceRow; 2] = [ServiceRow::of::<Quiet, crate::core_api::RealCore>("quiet", &[]), ServiceRow::of::<Refuser, crate::core_api::RealCore>("refuser", &[])];
    static ROWS_P: RowPtrs<2> = RowPtrs::of(&ROWS);

    #[test]
    fn the_services_refuse_after_the_generic_refusals_and_only_for_their_module() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        let table = ServiceTable::new(&ROWS_P.0, &REAL_CORE);
        let go = |f: Facts, owner: u8| unsafe { refusal_with_services(&p, &f, &table, owner) };
        assert_eq!(go(Facts::default(), 1), None);
        assert_eq!(go(Facts::default(), 2), Some(E_RELOAD_ROUTED));
        // a generic refusal comes first
        assert_eq!(go(Facts { menus: true, ..Facts::default() }, 2), Some(E_RELOAD_MENUS));
        p.play.set(Some(1));
        assert_eq!(go(Facts::default(), 2), Some(E_RELOAD_PLAYING));
    }
}
