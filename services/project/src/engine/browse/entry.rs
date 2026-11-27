//! `dk_project_entry`: one row of the project list (`dk_project_list`). The layout is the C struct of `<dk/platform/project.h>`.

/// The slot is the current project.
pub const ENTRY_CURRENT: u8 = 1;
/// The slot has digikit data (a `.dkp` file of the slot is in the store).
pub const ENTRY_DATA: u8 = 2;
/// Bit 1 is not known yet: the first listing of `projects/` has not ended.
pub const ENTRY_UNKNOWN: u8 = 4;

/// One project of the firmware's list.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProjectEntry {
    pub slot: u8,
    pub flags: u8,
    pub name_len: u8,
    pub name: [u8; 16],
}

const _: () = assert!(core::mem::size_of::<ProjectEntry>() == 19);
const _: () = assert!(core::mem::offset_of!(ProjectEntry, name) == 3);
