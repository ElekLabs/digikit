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
    /// An effect of the module is on a route (an fx module only).
    pub fx_routed: bool,
}

/// The refusal of a reload, or `None` when it may go on. One function, one order: safe mode, the firmware's transport (playing, paused, or not
/// readable), card operations, menus of the module, an effect on a route. The first that holds names the status.
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
    if f.fx_routed {
        return Some(E_RELOAD_ROUTED);
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
            fx_routed: true,
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
        assert_eq!(code(&p, f), Some(E_RELOAD_ROUTED));
        f.fx_routed = false;
        assert_eq!(code(&p, f), None);
    }
}
