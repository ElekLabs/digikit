//! `State`: all runtime state in one object a host can peek (`DKRT_STATE` in plan.json symbols), and the rules about it that the resident core and the system package both
//! use. The runtime owns the static; the package reaches it through a pointer of the resident table.
use crate::module::Module;
use crate::MAX_MODULES;
use dkrt_sdk::RUNTIME_VERSION;

pub const LOG_ENTRIES: usize = 16;
pub const STATE_MAGIC: u32 = 0x444b_5254; // "DKRT"

/// One entry of the runtime log in `State`: part of the layout the major version of the resident table covers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LogEntry {
    pub msg: [u8; 24],
    pub value: u32,
}

/// All runtime state, one object a host can peek (`DKRT_STATE` in plan.json symbols). The layout is part of what host tools read (symbol offsets) and, for the part the
/// package sees (`PackageState`), of the major version of the resident table (`resident.rs`). `A` is what sits where the arena keeps its own words: the resident core
/// has them (`ArenaBook`), the package gets them as opaque words (`ArenaWords`) and uses the arena through the table's `ArenaFns`.
#[repr(C)]
pub struct StateOf<A> {
    pub magic: u32,
    pub runtime: u32,
    pub ticks: u32,
    pub last_result: i32,
    pub loads: u32,
    pub tick_calls: u32,
    pub arena: A,
    pub log_next: u32,
    pub modules: [Module; MAX_MODULES],
    pub log: [LogEntry; LOG_ENTRIES],
}

/// The arena's own words in `State` (resident only; the arena owns them).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArenaBook {
    /// Head of the arena free list (address of the first free block, 0 = none).
    pub free_head: u32,
    pub lo: u32,
    pub hi: u32,
}

/// The arena's words as the package sees them: three words it does not read.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArenaWords([u32; 3]);

/// `DKRT_STATE`, whole: the resident core's view.
pub type State = StateOf<ArenaBook>;

/// What the package reaches through the resident table (`Views::state`): the same layout, without the arena's fields. Its layout is part of the major version.
pub type PackageState = StateOf<ArenaWords>;

/// Bit 31 of the `.dkm` header word at 0x30: the module was built with `DK_TRACE`; the other bits are the runtime version it needs.
pub const FLAG_TRACE: u32 = 1 << 31;

/// Whether this runtime loads a module that needs runtime version `needs` (the header word at 0x30, packed major << 16 | minor << 8 | patch,
/// the newest `since` of the imports it links; 0: it links none). 0.x semver: another major or minor than the runtime's refuses, and so does
/// a higher patch (a patch only adds). The trace flag is not part of the version.
pub fn runtime_accepts(needs: u32) -> bool {
    let needs = needs & !FLAG_TRACE;
    needs == 0 || (needs >> 8 == RUNTIME_VERSION >> 8 && needs & 255 <= RUNTIME_VERSION & 255)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    /// The package's view is the resident state with the arena words opaque: every field at the same offset, the whole the same size.
    #[test]
    fn the_package_view_is_the_state_layout() {
        assert_eq!(size_of::<ArenaBook>(), size_of::<ArenaWords>());
        assert_eq!(size_of::<State>(), size_of::<PackageState>());
        assert_eq!(offset_of!(State, magic), offset_of!(PackageState, magic));
        assert_eq!(
            offset_of!(State, tick_calls),
            offset_of!(PackageState, tick_calls)
        );
        assert_eq!(offset_of!(State, arena), offset_of!(PackageState, arena));
        assert_eq!(
            offset_of!(State, log_next),
            offset_of!(PackageState, log_next)
        );
        assert_eq!(
            offset_of!(State, modules),
            offset_of!(PackageState, modules)
        );
        assert_eq!(offset_of!(State, log), offset_of!(PackageState, log));
    }

    /// The arena words that the health proof of patchkit reads stay where they were: after the six words, `free_head`, `lo`, `hi`.
    #[test]
    fn the_arena_words_keep_their_offsets() {
        assert_eq!(offset_of!(State, arena), 24);
        assert_eq!(offset_of!(ArenaBook, lo), 4);
        assert_eq!(offset_of!(ArenaBook, hi), 8);
    }
}
