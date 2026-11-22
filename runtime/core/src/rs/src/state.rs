//! The runtime's state: sizes, the arena and staging buffers, `State` (one object a host can peek), the other `DKRT_*` statics that the
//! host tools read, and the runtime's own log ring.

use crate::platform::st;
use crate::{arena, dlog};
use dkrt_abi::module::Module;
use dkrt_sdk::*;

/// Arena bytes. A sub-region of `cf.expansion` that is part of this object's bss: patchkit places it with the object, so no
/// static object of any module can overlap it, and the reset-path loader zeroes it.
pub use dkrt_sdk::ARENA_BYTES;
/// Staging buffer for a `.dkm` file (and the store directory): files are at most this size in v0.
pub use dkrt_sdk::STAGE_BYTES;
pub use dkrt_abi::MAX_MODULES;
/// Records of the loader: the module records (`DKRT_STATE.modules`, `MAX_MODULES`) and the records of provider files, after them (`loader/records.rs`).
pub use dkrt_abi::REC_MAX;
pub const LOG_ENTRIES: usize = 16;
pub const STATE_MAGIC: u32 = 0x444b_5254; // "DKRT"

#[repr(C, align(16))]
pub struct Arena(pub [u8; ARENA_BYTES]);
#[repr(C, align(16))]
pub struct Stage(pub [u8; STAGE_BYTES]);

#[no_mangle]
pub static mut DKRT_ARENA: Arena = Arena([0; ARENA_BYTES]);
#[no_mangle]
pub static mut DKRT_STAGE: Stage = Stage([0; STAGE_BYTES]);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LogEntry {
    pub msg: [u8; 24],
    pub value: u32,
}

/// All runtime state, one object a host can peek (`DKRT_STATE` in plan.json symbols).
#[repr(C)]
pub struct State {
    pub magic: u32,
    pub runtime: u32,
    pub ticks: u32,
    pub last_result: i32,
    pub loads: u32,
    pub tick_calls: u32,
    /// Head of the arena free list (address of the first free block, 0 = none).
    pub free_head: u32,
    pub arena_lo: u32,
    pub arena_hi: u32,
    pub log_next: u32,
    pub modules: [Module; MAX_MODULES],
    pub log: [LogEntry; LOG_ENTRIES],
}

#[no_mangle]
pub static mut DKRT_STATE: State = unsafe { core::mem::zeroed() };
/// The store path each module record was loaded from (kept apart from `State`, whose layout the proof tools peek).
#[no_mangle]
pub static mut DKRT_PATHS: [[u8; 64]; MAX_MODULES] = [[0; 64]; MAX_MODULES];
/// The address of each module record's `menu` (0: none), apart from `State` for the same reason.
#[no_mangle]
pub static mut DKRT_MENUS: [u32; MAX_MODULES] = [0; MAX_MODULES];
/// The menu objects built for each module record (`dkrt_menu_built`): a menu keeps pointers into the module's code and data, so a module with one is not unloaded or
/// reloaded. Cleared when a module loads into the record.
#[no_mangle]
pub static mut DKRT_MENU_OBJS: [u8; MAX_MODULES] = [0; MAX_MODULES];

/// Was a menu object built for module record `rec` (`dkrt_menu_built`)?
pub(crate) unsafe fn menus_held(rec: usize) -> bool {
    rec < dkrt_abi::MAX_MODULES && core::ptr::read_volatile(core::ptr::addr_of!(DKRT_MENU_OBJS[rec])) != 0
}

/// Bit 31 of the `.dkm` header word at 0x30: the module was built with `DK_TRACE`; the other bits are the runtime version it needs.
pub const FLAG_TRACE: u32 = 1 << 31;
/// Whether this runtime loads a module that needs runtime version `needs` (the header word at 0x30, packed major << 16 | minor << 8 | patch,
/// the newest `since` of the imports it links; 0: it links none). 0.x semver: another major or minor than the runtime's refuses, and so does
/// a higher patch (a patch only adds). The trace flag is not part of the version.
pub fn runtime_accepts(needs: u32) -> bool {
    let needs = needs & !FLAG_TRACE;
    needs == 0 || (needs >> 8 == RUNTIME_VERSION >> 8 && needs & 255 <= RUNTIME_VERSION & 255)
}
/// The runtime version each module record needs (`MODULES` reports it).
#[no_mangle]
pub static mut DKRT_MODULE_RUNTIME: [u32; MAX_MODULES] = [0; MAX_MODULES];
#[no_mangle]
pub static mut DKRT_MAILBOX: Mailbox = Mailbox {
    cmd: 0,
    seq: 0,
    result: 0,
    arg: [0; 96],
};

/// Functions of `dkrt_poll_add`, called once per UI-loop pass (the DIGIKIT menu's HEALTH page and `core/ui`'s `dk_ui_tick` timers). The
/// proofs read the table.
pub const POLLS: usize = 4;
#[no_mangle]
pub static mut DKRT_POLLS: [usize; POLLS] = [0; POLLS];
/// The owner of each poll slot, set by `dkrt_poll_add`: the module record whose loaded range holds the function, or `POLL_OWNER_RUNTIME` for a
/// function outside every module (the firmware image: the patched-in core modules). Meaningful while the slot's function is not 0.
#[no_mangle]
pub static mut DKRT_POLL_OWNER: [u8; POLLS] = [POLL_OWNER_RUNTIME; POLLS];
pub const POLL_OWNER_RUNTIME: u8 = 0xff;

/// Functions of `dkrt_unload_observer_add`: called with the owner of each content module that is unloaded, after its `fini`.
pub const OBSERVERS: usize = 4;
#[no_mangle]
pub static mut DKRT_UNLOAD_OBSERVERS: [usize; OBSERVERS] = [0; OBSERVERS];

pub unsafe fn log(msg: *const u8, value: u32) {
    log_at(logk::L_INFO as u32, msg, value);
}

/// `log` at `level` (0 is an error event; above 4 is 4).
pub unsafe fn log_at(level: u32, msg: *const u8, value: u32) {
    let level = if level > logk::L_TRACE as u32 { logk::L_TRACE } else { level as u16 };
    let s = st();
    let e = &mut s.log[(s.log_next as usize) % LOG_ENTRIES];
    let mut i = 0;
    while i < 23 && *msg.add(i) != 0 {
        e.msg[i] = *msg.add(i);
        i += 1;
    }
    while i < 24 {
        e.msg[i] = 0;
        i += 1;
    }
    e.value = value;
    s.log_next = s.log_next.wrapping_add(1);
    dlog::ev(
        logk::MOD_LOG,
        (level << logk::LEVEL_SHIFT) | if level == logk::L_ERROR { logk::F_ERROR } else { 0 },
        [dlog::word_of(e.msg.as_ptr(), 0, 24), dlog::word_of(e.msg.as_ptr(), 4, 24), dlog::word_of(e.msg.as_ptr(), 8, 24), dlog::word_of(e.msg.as_ptr(), 12, 24), value],
    );
}

/// First-use setup: arena free list, magic.
pub unsafe fn init_once() {
    let s = st();
    if s.magic == STATE_MAGIC {
        return;
    }
    crate::dev::valid(); // checks and keeps the device object's validity (an invalid object has no capability)
    arena::init();
    // the in-crate services declare their record kinds and the runtime compares them with the generated table (declare-check-commit)
    crate::handlers::check_declarations();
    s.runtime = RUNTIME_VERSION;
    s.magic = STATE_MAGIC;
}
