//! The public C ABI of the runtime: load, poll functions, module and effect listings, the selection, boot info and restart.

use crate::platform::st;
use crate::platform::PLATFORM;
use dkrt_abi::platform::Platform;
use crate::state::{init_once, DKRT_UNLOAD_OBSERVERS, OBSERVERS, DKRT_POLLS, DKRT_POLL_OWNER, MAX_MODULES, POLLS, POLL_OWNER_RUNTIME, DKRT_MENUS, DKRT_MENU_OBJS, DKRT_PATHS};
use crate::{boot, dlog, selection, settings};
use crate::hooks;
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_sdk::*;

// ---- the public C ABI ----------------------------------------------------------------------------------------------------

/// Load the module file `path` (NUL-terminated, relative to the store root, for example "modules/dyn-demo.dkm").
/// Returns 0 or a negative `E_*` code; on an error nothing was left behind. Task context only; not reentrant. Only the boot hook
/// loads: the module set is fixed per session, so every other caller gets `E_BOOT_ONLY` (hot reload and `dkrt_unload` are archived,
/// tag `digikit-hot-reload-v1`, docs/plans/digikit-hot-reload.md).
#[no_mangle]
pub unsafe extern "C" fn dkrt_load(path: *const u8) -> i32 {
    init_once();
    let r = hooks::module_load(path);
    st().last_result = r;
    r
}



/// Add a function that `dkrt_tick` calls once per UI-loop pass, in the UI task (`core/ui` keeps its `dk_ui_tick` timers with one). Adding one
/// that is in the table is OK; `E_NOMEM` when it is full. Task context.
#[no_mangle]
pub unsafe extern "C" fn dkrt_poll_add(f: usize) -> i32 {
    if f == 0 {
        return E_CONTRACT;
    }
    let t = &mut *addr_of_mut!(DKRT_POLLS);
    let mut free = POLLS;
    let mut i = 0;
    while i < POLLS {
        if t[i] == f {
            return OK;
        }
        if t[i] == 0 && free == POLLS {
            free = i;
        }
        i += 1;
    }
    if free == POLLS {
        return E_NOMEM;
    }
    t[free] = f;
    (*addr_of_mut!(DKRT_POLL_OWNER))[free] = poll_owner(f);
    OK
}

/// Add an unload observer (`dkrt_unload_observer_add`). Task context.
#[no_mangle]
pub unsafe extern "C" fn dkrt_unload_observer_add(f: usize) -> i32 {
    if f == 0 {
        return E_CONTRACT;
    }
    let t = &mut *addr_of_mut!(DKRT_UNLOAD_OBSERVERS);
    let mut free = OBSERVERS;
    let mut i = 0;
    while i < OBSERVERS {
        if t[i] == f {
            return OK;
        }
        if t[i] == 0 && free == OBSERVERS {
            free = i;
        }
        i += 1;
    }
    if free == OBSERVERS {
        return E_NOMEM;
    }
    t[free] = f;
    OK
}

/// Remove an unload observer (no effect when it is not there). Task context.
#[no_mangle]
pub unsafe extern "C" fn dkrt_unload_observer_remove(f: usize) {
    let t = &mut *addr_of_mut!(DKRT_UNLOAD_OBSERVERS);
    let mut i = 0;
    while i < OBSERVERS {
        if f != 0 && t[i] == f {
            t[i] = 0;
        }
        i += 1;
    }
}

/// The owner id of a module context: its record index; `E_CONTRACT` for a pointer that is not the live context of a content module (a service has none given out).
#[no_mangle]
pub unsafe extern "C" fn dkrt_ctx_owner(ctx: usize) -> i32 {
    hooks::ctx_owner(ctx)
}

/// What the running module code is (`dlog::PHASE_*`); 0 outside module code. UI task.
#[no_mangle]
pub unsafe extern "C" fn dkrt_phase() -> u32 {
    crate::dlog::current_phase() as u32
}

/// Tell every observer that the content module record `owner` is unloaded: after its `fini` and the drop of its polls, before its code is freed. An observer that lies in the unloaded
/// module `[base, base + size)` is dropped first (it would call into freed code).
pub(crate) unsafe fn notify_unload(owner: usize, base: u32, size: u32) {
    let t = &mut *addr_of_mut!(DKRT_UNLOAD_OBSERVERS);
    let mut i = 0;
    while i < OBSERVERS {
        if t[i] != 0 && in_range(t[i] as u32, base, size) {
            t[i] = 0;
        }
        i += 1;
    }
    i = 0;
    while i < OBSERVERS {
        let f = t[i];
        if f != 0 {
            let call: unsafe extern "C" fn(u32) = core::mem::transmute(f);
            call(owner as u32);
        }
        i += 1;
    }
}

/// A menu object was built for the module that holds `code`, else `data` (`dkrt_menu_built`): that module has one more menu, so it is not unloaded or reloaded
/// (`reload::refusal`). Neither address in a module (a menu of the firmware image, of the DIGIKIT menu): no effect. UI task.
#[no_mangle]
pub unsafe extern "C" fn dkrt_menu_built(code: usize, data: usize) {
    if let Some(rec) = menu_owner(code, data) {
        let n = &mut (*addr_of_mut!(DKRT_MENU_OBJS))[rec];
        *n = n.saturating_add(1);
    }
}

/// The matching undo of `dkrt_menu_built` (`dkrt_menu_release`): the module that holds `code`, else `data`, has one menu less; the count stops at 0, so a release after a load
/// into the record (which starts the count at 0) changes nothing. UI task.
#[no_mangle]
pub unsafe extern "C" fn dkrt_menu_release(code: usize, data: usize) {
    if let Some(rec) = menu_owner(code, data) {
        let n = &mut (*addr_of_mut!(DKRT_MENU_OBJS))[rec];
        *n = n.saturating_sub(1);
    }
}

/// The module record that owns a menu of `code`, else of `data`; `None` when neither address is in a module.
unsafe fn menu_owner(code: usize, data: usize) -> Option<usize> {
    let mut rec = poll_owner(code);
    if rec == POLL_OWNER_RUNTIME {
        rec = poll_owner(data);
    }
    (rec != POLL_OWNER_RUNTIME).then_some(rec as usize)
}

/// Is `f` in `[base, base + size)`? In the width of an address: the 64-bit difference of two 32-bit addresses gave the wrong answer on the device for an address below `base`
/// (the system package in its own block counted as inside the first module's range, so its menus were counted for that module and its polls dropped with it).
#[inline(always)]
pub(crate) fn in_range(f: u32, base: u32, size: u32) -> bool {
    f.wrapping_sub(base) < size
}

/// The module record whose loaded range holds `f`, or `POLL_OWNER_RUNTIME`.
unsafe fn poll_owner(f: usize) -> u8 {
    let s = st();
    let mut rec = 0;
    while rec < MAX_MODULES {
        let m = &s.modules[rec];
        if m.used != 0 && in_range(f as u32, m.base, m.size) {
            return rec as u8;
        }
        rec += 1;
    }
    POLL_OWNER_RUNTIME
}

/// Free the poll slots whose function lies in `[base, base + size)` (a module that is released).
pub(crate) unsafe extern "C" fn poll_drop_range(base: u32, size: u32) {
    let t = &mut *addr_of_mut!(DKRT_POLLS);
    let mut i = 0;
    while i < POLLS {
        if t[i] != 0 && in_range(t[i] as u32, base, size) {
            t[i] = 0;
            (*addr_of_mut!(DKRT_POLL_OWNER))[i] = POLL_OWNER_RUNTIME;
        }
        i += 1;
    }
}

/// Remove a function added with `dkrt_poll_add` (no effect when it is not there). Task context.
#[no_mangle]
pub unsafe extern "C" fn dkrt_poll_remove(f: usize) {
    let t = &mut *addr_of_mut!(DKRT_POLLS);
    let mut i = 0;
    while i < POLLS {
        if f != 0 && t[i] == f {
            t[i] = 0;
            (*addr_of_mut!(DKRT_POLL_OWNER))[i] = POLL_OWNER_RUNTIME;
        }
        i += 1;
    }
}



/// The header fields of the `.dkm` at store sector `first` (`DkrtScan.first`) into `out`, for the INFO page of the DIGIKIT menu: 0, `E_FORMAT` when
/// the file is not a `.dkm`, or the read error. The runtime reads the sector into its own DMA-safe buffer. Task context.
#[no_mangle]
pub unsafe extern "C" fn dkrt_module_header(first: u32, out: *mut DkrtModuleHeader) -> i32 {
    init_once();
    hooks::module_header(first, out)
}



/// The `index`th loaded module (in load-slot order) into `out`; returns 0, or `E_NOT_FOUND` past the end.
#[no_mangle]
pub unsafe extern "C" fn dkrt_module(index: u32, out: *mut DkrtModule) -> i32 {
    init_once();
    let s = st();
    let mut seen = 0;
    let mut i = 0;
    while i < MAX_MODULES {
        let m = &s.modules[i];
        if m.used != 0 {
            if seen == index {
                let o = &mut *out;
                o.name = m.name;
                o.version = m.version;
                o.base = m.base;
                o.size = m.size;
                o.path = (*addr_of!(DKRT_PATHS))[i];
                o.state = if m.used == 2 { MOD_LOADING } else { MOD_LOADED };
                let extra = hooks::module_extra(i);
                o.dsp_size = extra.dsp_size;
                o.menu = (*addr_of!(DKRT_MENUS))[i];
                o.ctx = extra.ctx;
                return OK;
            }
            seen += 1;
        }
        i += 1;
    }
    E_NOT_FOUND
}

/// The `index`th loaded effect that has a page (row order) into `out`: its tab, module id, allowed routes, default route, the route it runs on and the route the settings
/// give it (DIGIKIT > FX ROUTES). 0, or `E_NOT_FOUND` past the end. UI task.
#[no_mangle]
pub unsafe extern "C" fn dkrt_fx(index: u32, out: *mut DkrtFx) -> i32 {
    init_once();
    hooks::fx_entry(index, out)
}



// ---- boot-time loading (finding 37) ---------------------------------------------------------------------------------------

/// `dkrt_selection_get(out)`: the saved selection (what the next boot loads). Always 0.
#[no_mangle]
pub unsafe extern "C" fn dkrt_selection_get(out: *mut DkrtSelection) -> i32 {
    init_once();
    selection::selection_get(out)
}

/// `dkrt_selection_set(sel)`: save the selection (two copies, sequence number, CRC); no effect until the next boot. 0 or `E_SEL_*`.
#[no_mangle]
pub unsafe extern "C" fn dkrt_selection_set(sel: *const DkrtSelection) -> i32 {
    init_once();
    selection::selection_set(sel)
}

/// `dkrt_boot_info(out)`: safe mode and why, the boot counter, each selected module's result at this boot.
#[no_mangle]
pub unsafe extern "C" fn dkrt_boot_info(out: *mut DkrtBootInfo) {
    init_once();
    boot::boot_info(out)
}

/// `dkrt_boot_name(index, out)`: the file name held by selection index `index` (28 bytes, NUL padded).
#[no_mangle]
pub unsafe extern "C" fn dkrt_boot_name(index: u32, out: *mut u8) -> i32 {
    init_once();
    boot::name_copy(index as usize, out)
}

/// What `dkrt_restart` does to keep the working state (finding 40): bit 0 serializes the live project into the persistent arena
/// (`dkfw::project::serialize`), bit 1 writes the arena to the next boot record (`dkfw::storage::boot_record_write`, the firmware's own writer for
/// the OS-update path). The default is 0 (no save; the menu asks for a project save first): finding 40 has no edit that the save keeps,
/// and the serialize may race the firmware's BgWorker. 3 runs both steps, for the experiments of finding 40 (a host poke).
#[no_mangle]
pub static mut DKRT_SAVE_MODE: Aligned4 = Aligned4(0);

/// A word that is 4-aligned (the m68k ABI aligns `u32` to 2; the host pokes this one with aligned stores).
#[repr(C, align(4))]
pub struct Aligned4(pub u32);

/// Save the working state the way the firmware does before it flashes an update: the project into the arena, the arena into
/// the boot record. Runs in the UI task with interrupts enabled (the writer takes the eMMC mutex itself).
unsafe fn save_working_state() {
    let mode = read_volatile(addr_of!(DKRT_SAVE_MODE.0));
    if mode & 1 != 0 {
        crate::platform::project_serialize();
    }
    if mode & 2 != 0 {
        crate::platform::boot_record_write();
    }
}

/// MCF5441x eSDHC: offset of the present-state register (`PRSSTAT`); bits 0 to 2 are command inhibit, data inhibit and data line active.
const MCF5441X_ESDHC_PRSSTAT: u32 = 0x24;
/// Is a card transfer in flight? The card's eSDHC controller is the profile's (`[board] esdhc_base` of the device file); a device without one has no busy card.
pub(crate) unsafe extern "C" fn card_busy() -> bool {
    let p = crate::platform::profile();
    p.has_sdhc && read_volatile((p.esdhc_base + MCF5441X_ESDHC_PRSSTAT) as *const u32) & 7 != 0
}

/// `dkrt_restart()`: write pending settings (`settings::flush_if_dirty`), save the working state (`save_working_state`), then the firmware's own soft reset: interrupts masked, console drained,
/// DDR word 0 cleared, `0x80` into `RCM.RCR`. Returns only when it refuses (`E_CARD_BUSY`: a card transfer is in flight; the
/// check is made again with interrupts masked, when no other task can start one).
#[no_mangle]
pub unsafe extern "C" fn dkrt_restart() -> i32 {
    init_once();
    if card_busy() {
        return E_CARD_BUSY;
    }
    // a settings change younger than the write delay would be lost to the reset: write it now (a failure is in the log; the restart goes on)
    settings::flush_if_dirty();
    dlog::ev(logk::MENU, dlog::INFO, [logk::MENU_RESTART, 0, 0, 0, 0]);
    save_working_state();
    let sr = PLATFORM.irq_mask();
    if card_busy() {
        PLATFORM.irq_restore(sr);
        return E_CARD_BUSY;
    }
    PLATFORM.soft_reset()
}
