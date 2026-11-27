//! The table `rows_info` of the rows_info adapter (its word indexes) and the state of the machines registry (`RegState`, the owner of each reserved row and the strings the rows use).
//! The machines service owns the state; the parameter row writers read the words.
use dkrt_sdk::*;

const MAX_MACH: usize = limits::MACHINE_ROWS;
const MAX_PARAM: usize = limits::PARAM_ROWS;
const MAX_PAGE: usize = limits::PAGE_ROWS;
/// The longest machine or page text of a run-time row.
pub const MACH_TEXT: usize = 24;
const REP_SIZE: usize = 48;

/// Words of `rows_info` (byte offsets / 4; the layout is in `runtime/platform/rows/src/cf/rows-info.inc`). Words 48, 49 and 61 to 64 are not used: they were the machine selection view and
/// the in-use functions of hot reload (archived, docs/plans/digikit-hot-reload.md); the table keeps them as zeros so the other offsets stay.
pub mod w {
    pub const MAGIC: usize = 0;
    pub const DEVICE: usize = 2;
    pub const M_ROWS: usize = 3;
    pub const M_ROW_SIZE: usize = 4;
    pub const M_FIRST: usize = 5;
    pub const M_COUNT: usize = 6;
    pub const M_DYN0: usize = 7;
    pub const M_DYN_N: usize = 8;
    pub const NAMES: usize = 9;
    pub const PERMIT: usize = 10;
    pub const PERTYPE: usize = 11;
    pub const O_LONG: usize = 12;
    pub const O_SHORT: usize = 13;
    pub const O_F0: usize = 14;
    pub const O_CLONE: usize = 15;
    pub const O_POS: usize = 16;
    pub const O_OWN: usize = 17;
    pub const O_NAME: usize = 18;
    pub const O_SNAME: usize = 19;
    pub const P_TABLE: usize = 20;
    pub const P_ROWS: usize = 21;
    pub const P_ROW_SIZE: usize = 22;
    pub const P_FIRST: usize = 23;
    pub const P_DYN0: usize = 25;
    pub const P_DYN_N: usize = 26;
    pub const PF_SIZE: usize = 27;
    pub const P_TEMPLATE_OFF: usize = 28;
    pub const EMPTY: usize = 29;
    pub const SPEC: usize = 30; // 15 words
    pub const KNOBS: usize = 45;
    pub const SLOT_BASE: usize = 46;
    pub const RESERVE_TEMPLATE: usize = 47;
    pub const PG_ROWS: usize = 50; // 0: the device keeps its knobs in the machine row
    pub const PG_ROW_SIZE: usize = 51;
    pub const PG_FIRST: usize = 52;
    pub const PG_DYN0: usize = 53;
    pub const PG_DYN_N: usize = 54;
    pub const PGO_NAME: usize = 55;
    pub const PGO_TITLE: usize = 56;
    pub const PGO_C: usize = 57;
    pub const PGO_MACH: usize = 58;
    pub const PGO_S: usize = 59;
    pub const PG_FREE: usize = 60;
    pub const VERSION: usize = 1;
    // the builder profile (version 3): words 65 to 86
    pub const P_CLONE: usize = 65;
    pub const P_LEVEL: usize = 66;
    pub const P_OWN: usize = 67;
    pub const P_TUNE: usize = 68;
    pub const P_TUNE_SLOT: usize = 69;
    pub const P_LEVEL_CODE: usize = 70;
    pub const P_LEVEL_KNOB: usize = 71;
    pub const P_NSLOTS: usize = 72;
    pub const P_SLOTS: usize = 73; // 8 words
    pub const P_TPL: usize = 81; // 6 words
                                 // what the machines service needs of the registry besides the rows
    pub const M_ENTRIES: usize = 87; // machines.entries
    pub const SLOTMAP: usize = 88; // the descriptor field of mirror index 25 .. 34 (-1: none); 0 where the device has none
    pub const READER_REAL: usize = 89; // the copy of the stock type reader; 0 on a device without a hooked reader
    pub const LIST_STOCK: usize = 90; // the stock display order of the machine list: `M_FIRST` words, one type each
    pub const PG_COUNT: usize = 91; // pages.count
                                    // the parameter blocks (the device file's `[[rows.block]]`): the number of blocks, then owner, first code, rows and template of a free row for each
    pub const BLOCKS: usize = 92;
    pub const BLOCK_0: usize = 93;
    pub const RESERVED: usize = 105; // reserved parameter rows, all blocks
    pub const CAPS: usize = 106; // the four capacity words
    pub const WORDS: usize = 110;
}

#[repr(C, align(4))]
pub struct Strings {
    pub mach_name: [[u8; MACH_TEXT]; MAX_MACH],
    pub mach_short: [[u8; MACH_TEXT]; MAX_MACH],
    pub rep_long: [[u8; REP_SIZE]; MAX_MACH],
    pub rep_short: [[u8; REP_SIZE]; MAX_MACH],
    pub param_name: [[u8; 32]; MAX_PARAM],
    pub param_short: [[u8; 16]; MAX_PARAM],
    pub page_name: [[u8; MACH_TEXT]; MAX_PAGE],
    pub page_title: [[u8; MACH_TEXT]; MAX_PAGE],
    pub rep_pname: [[u8; REP_SIZE]; MAX_PAGE],
    pub rep_ptitle: [[u8; REP_SIZE]; MAX_PAGE],
}

/// The registry's state that only the owner of the machines service reads: who owns each reserved parameter and page row (module record index + 1, 0 = free) and the strings
/// the rows use. The owner of each machine row is not here: it is read outside the owner (the MODULES reply), so it is a `Shared` cell of the service (`registry::MACH_OWNER`).
#[repr(C, align(4))]
pub struct RegState {
    pub magic: u32,
    pub param_owner: [u8; MAX_PARAM],
    pub page_owner: [u8; MAX_PAGE],
    /// An empty std::string representation (length 0, capacity 0, reference count -1, NUL) for a free machine row.
    pub empty_rep: [u32; 4],
    pub s: Strings,
}
