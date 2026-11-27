//! The contract between the system package and a package part (`patchkit syspkg build --part`): a part is an object linked into `system.dksp` that the package does not name.
//!
//! The package owns a directory of `MAX_PARTS` slots (`DKPKG_PARTS`, `package/parts.rs`). `patchkit` writes the address of each part's `DKPART_TABLE` into the next free slot
//! when it links the part, so the package finds a part through the directory and never by a symbol of the part. A part's table gives its name hash and three entries: `start`,
//! `step` and `stop`. Every call runs in the UI task and does not block.

/// The word that starts the package's directory of parts (`DKPS`); the directory is in the image's data, so a tool can find it by this word.
pub const PARTS_MAGIC: u32 = 0x444b_5053;
/// The word that starts a part's table (`DKPT`).
pub const TABLE_MAGIC: u32 = 0x444b_5054;
/// Parts one package takes.
pub const MAX_PARTS: usize = 4;
/// The symbol of a part's table; `patchkit` renames it per slot.
pub const TABLE_SYMBOL: &str = "DKPART_TABLE";
/// The symbol of the package's directory.
pub const DIRECTORY_SYMBOL: &str = "DKPKG_PARTS";

/// What a part sees of the package during a call.
#[repr(C)]
pub struct PartEnv {
    /// Bytes of this record.
    pub size: u32,
    /// The words of the image's info table entry `name_hash` (`CoreApi::info`); null when the image has none.
    pub info: unsafe extern "C" fn(name_hash: u32) -> *const u32,
}

/// A part's entries. `step` and `stop` answer 0 when done and 1 to be called again (a part never blocks); a negative value is an error.
#[repr(C)]
pub struct PartTable {
    /// `TABLE_MAGIC`.
    pub magic: u32,
    /// The `dkneeds::keys::name_hash` of the part's name.
    pub name_hash: u32,
    pub start: unsafe extern "C" fn(env: *const PartEnv) -> i32,
    pub step: unsafe extern "C" fn(env: *const PartEnv) -> i32,
    pub stop: unsafe extern "C" fn(env: *const PartEnv) -> i32,
}

/// The package's directory of parts: the magic, then one table pointer per part (null: free).
#[repr(C)]
pub struct PartDirectory {
    pub magic: u32,
    pub tables: [*const PartTable; MAX_PARTS],
}

// the directory is read-only data once linked
unsafe impl Sync for PartDirectory {}
unsafe impl Sync for PartTable {}
