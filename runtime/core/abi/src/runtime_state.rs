//! `State`: all runtime state in one object a host can peek (`DKRT_STATE` in plan.json symbols), and the rules about it that the resident core and the system package both
//! use. The runtime owns the static; the package reaches it through a pointer of the resident table.
use crate::module::Module;
use crate::MAX_MODULES;
use dkrt_sdk::RUNTIME_VERSION;

pub const LOG_ENTRIES: usize = 16;
pub const STATE_MAGIC: u32 = 0x444b_5254; // "DKRT"

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

/// Bit 31 of the `.dkm` header word at 0x30: the module was built with `DK_TRACE`; the other bits are the runtime version it needs.
pub const FLAG_TRACE: u32 = 1 << 31;

/// Whether this runtime loads a module that needs runtime version `needs` (the header word at 0x30, packed major << 16 | minor << 8 | patch,
/// the newest `since` of the imports it links; 0: it links none). 0.x semver: another major or minor than the runtime's refuses, and so does
/// a higher patch (a patch only adds). The trace flag is not part of the version.
pub fn runtime_accepts(needs: u32) -> bool {
    let needs = needs & !FLAG_TRACE;
    needs == 0 || (needs >> 8 == RUNTIME_VERSION >> 8 && needs & 255 <= RUNTIME_VERSION & 255)
}
