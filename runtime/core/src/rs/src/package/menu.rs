//! The DIGIKIT menu (C, `runtime/apps/digikit-menu`) as the package sees it: one entry, `dk_app_settings_rows`, which adds the menu's row to the stock SETTINGS menu. The package artifact
//! links the C object (`patchkit syspkg build`), so the symbol is the menu's own. The host tests link no C: there the entry does nothing (the menu itself is tested on the host with
//! fakes of the runtime, `runtime/platform/ui/host`).

use core::ptr::addr_of;
use dkrt_abi::dev_addr::DevAddr;

/// The statics of the menu that the host tests and tools read (`MENU_*` ids of the host directory, abi.toml): in the package artifact they are the C object's globals, in a host
/// test binary (which links no C) a stand-in byte each, so the directory has an address for every id of this package.
macro_rules! menu_static {
    ($name:ident) => {
        #[cfg(feature = "package-image")]
        extern "C" {
            static $name: u8;
        }
        #[cfg(not(feature = "package-image"))]
        static $name: u8 = 0;
    };
}

macro_rules! menu_statics {
    ($(($id:ident, $name:ident)),* $(,)?) => {
        $(menu_static!($name);)*

        /// The device address of the menu static that host directory id `id` stands for; `None` for any other id. An if chain, not a `match` (a jump table is not ColdFire code).
        #[allow(unused_unsafe)]
        pub(crate) fn published(id: u32) -> Option<DevAddr> {
            $(if id == dkrt_sdk::$id {
                return Some(DevAddr::of_published(unsafe { addr_of!($name) }));
            })*
            None
        }
    };
}

menu_statics! {
    (HOST_MENU_LOAD, DK_LOAD),
    (HOST_MENU_LOAD_N, DK_LOAD_N),
    (HOST_MENU_POPUP, DK_POPUP),
    (HOST_MENU_POPUP_TEXT, DK_POPUP_TEXT),
    (HOST_MENU_POPUPS, DK_POPUPS),
    (HOST_MENU_ABOUT, DK_ABOUT),
    (HOST_MENU_ABOUT_N, DK_ABOUT_N),
    (HOST_MENU_PANEL, DK_PANEL),
    (HOST_MENU_PANEL_N, DK_PANEL_N),
    (HOST_MENU_MEM, DK_MEM),
    (HOST_MENU_MEM_N, DK_MEM_N),
    (HOST_MENU_TICK, TICK),
    (HOST_MENU_LOADED, LOADED),
    (HOST_MENU_LINES, DK_LINES),
    (HOST_MENU_REDRAWS, DK_REDRAWS),
    (HOST_MENU_POLLS, DK_POLLS),
    (HOST_MENU_HEALTH, DK_HEALTH),
    (HOST_MENU_HEALTH_MENU, DK_HEALTH_MENU),
    (HOST_MENU_FXR_PEND, DK_FXR_PEND),
    (HOST_MENU_FXR_SET, DK_FXR_SET),
    (HOST_MENU_FXR_FLUSH, DK_FXR_FLUSH),
    (HOST_MENU_APPLIES, DK_APPLIES),
    (HOST_MENU_ROUTE_N, DK_ROUTE_N),
    (HOST_MENU_ROUTE_ITEM, DK_ROUTE_ITEM),
}

#[cfg(feature = "package-image")]
extern "C" {
    /// The menu's hook function: add the DIGIKIT row to the stock menu `settings_menu` (`dk_ui_stock_row`).
    pub(crate) fn dk_app_settings_rows(settings_menu: *mut u8);
}

#[cfg(not(feature = "package-image"))]
pub(crate) unsafe extern "C" fn dk_app_settings_rows(settings_menu: *mut u8) {
    #[cfg(test)]
    tests::ASKED.store(settings_menu as usize, core::sync::atomic::Ordering::SeqCst);
    #[cfg(not(test))]
    let _ = settings_menu;
}

/// What the host tests see of the entry: the menu pointer it was last called with (0: not called since `take`).
#[cfg(test)]
pub(crate) mod tests {
    use core::sync::atomic::{AtomicUsize, Ordering};

    pub(super) static ASKED: AtomicUsize = AtomicUsize::new(0);

    /// The menu the entry was called with since the last `take`, 0 for none.
    pub(crate) fn take() -> usize {
        ASKED.swap(0, Ordering::SeqCst)
    }
}
