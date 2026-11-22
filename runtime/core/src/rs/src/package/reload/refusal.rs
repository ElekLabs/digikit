//! The refusal of a live job before anything changes: what the caller knows (`Facts`) put in order.
use super::*;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_sdk::*;

/// What the caller knows about the system when a reload is asked for. Each field is read where it lives (the loader, the card, the module's
/// records); `refusal` only puts the answers in order.
#[derive(Clone, Copy, Default)]
pub struct Facts {
    /// This boot is in safe mode.
    pub safe_mode: bool,
    /// An upload is open, or a store write is in flight (`E_STATE`): the files of the store are not settled.
    pub upload_open: bool,
    /// A card transfer or a store job is in flight.
    pub card_busy: bool,
    /// A menu object was built for the module (`dkrt_menu_built`: it called `dk_ui_menu_new`, or the DIGIKIT menu holds it while its SETTINGS page is open until `dkrt_menu_release`; the DIGIKIT menu's own menus count for no content module): the menu keeps
    /// pointers into the module's code and data.
    pub menus: bool,
}

impl Facts {
    /// The facts of the system for a job on module record `rec` (`None`: a load, which has no module yet), with the card transfer flag the caller read.
    pub(crate) unsafe fn of(rec: Option<usize>, card_busy: bool) -> Facts {
        Facts {
            safe_mode: crate::boot::info().safe != 0,
            upload_open: crate::store_writer::is_open() || crate::store_writer::is_locked(),
            card_busy,
            menus: rec.is_some_and(|r| crate::state::menus_held(r)),
        }
    }
}

/// The refusal of a job on module record `rec` (`None` for a load): the generic ones (`refusal` over `Facts::of`), then for a module those of its services (`service`).
pub(crate) unsafe fn refusal_for<P: PackagePlatform>(p: &P, rec: Option<usize>, card_busy: bool, service: impl FnOnce(usize) -> Option<i32>) -> Option<i32> {
    match (refusal(p, &Facts::of(rec, card_busy)), rec) {
        (None, Some(r)) => service(r),
        (generic, _) => generic,
    }
}

/// The refusal of a live job (a load, an unload or a reload), or `None` when it may go on. One function, one order: safe mode, the firmware's transport (playing, paused, or not
/// readable), an open upload, card operations, menus of the module (`Facts::menus`: false for a load, which has no module yet). The first that holds names the status.
/// The services' own refusals (`Service::teardown_refusal`: a service that cannot give back the module's state now) come after it, for an unload and a reload: the manager asks them
/// (`RtHost::can_stop`).
pub unsafe fn refusal<P: PackagePlatform>(p: &P, f: &Facts) -> Option<i32> {
    if f.safe_mode {
        return Some(E_SAFE_MODE);
    }
    match p.play_state() {
        Some(0) => {}
        _ => return Some(E_RELOAD_PLAYING),
    }
    if f.upload_open {
        return Some(dkrt_sdk::proto::E_STATE);
    }
    if f.card_busy {
        return Some(E_CARD_BUSY);
    }
    if f.menus {
        return Some(E_RELOAD_MENUS);
    }
    None
}
