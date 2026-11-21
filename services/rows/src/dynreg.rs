//! The table `dynreg_info` of the dynreg adapter (its word indexes) and the state of the machines registry (`RegState`, the owner of each reserved row and the strings the rows use).
//! The machines service owns the state; the parameter row writers read the words.
use dkrt_sdk::*;

const MAX_MACH: usize = limits::MACHINE_ROWS;
const MAX_PARAM: usize = limits::PARAM_ROWS;
const MAX_PAGE: usize = limits::PAGE_ROWS;
/// The longest machine or page text of a run-time row.
pub const MACH_TEXT: usize = 24;
const REP_SIZE: usize = 48;

/// First version of `dynreg_info` with the filter list builder.
pub const FILTER_VIEW_VERSION: u32 = 4;

/// Words of `dynreg_info` (byte offsets / 4). Words 61 to 64 are reserved for the archived in-use scan.
pub mod w {
    pub const MAGIC: usize = 0;
    pub const DEVICE: usize = 2;
    pub const M_ROWS: usize = 3;
    pub const M_ROW_SIZE: usize = 4;
    pub const M_FIRST: usize = 5;
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
    pub const VIEW_VAR: usize = 48; // the address of the variable that holds the machine selection view (0 until it exists)
    pub const VIEW_BUILD: usize = 49; // the function that builds that view's items: build(view)
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
    pub const FILTER_VIEW_BUILD: usize = 87; // the filter list builder on the same view (version 4)
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

/// The registry's state: who owns each reserved row (module record index + 1, 0 = free) and the strings the rows use.
#[repr(C, align(4))]
pub struct RegState {
    pub magic: u32,
    pub mach_owner: [u8; MAX_MACH],
    pub param_owner: [u8; MAX_PARAM],
    pub page_owner: [u8; MAX_PAGE],
    /// An empty std::string representation (length 0, capacity 0, reference count -1, NUL) for a free machine row.
    pub empty_rep: [u32; 4],
    pub s: Strings,
}
