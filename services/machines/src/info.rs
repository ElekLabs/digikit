//! The layout of the image table `machines_info` of `core/machines` (its `[[info]]`): what the machines service reads of the patched image. The service file copies the
//! table into its own bss for its assembler hooks (`dks/src/image.rs`); the crate reads the native page functions from the table the core gives (`CoreApi::info`).
//!
//! The words are big endian in the image (ColdFire is big endian, so a read of the word is the value). A word that is 0 was not bound: the service does not call it.

/// `MACH`.
pub const MAGIC: u32 = 0x4d41_4348;
/// Version 4 appends the adapter's native filter graph function.
pub const VERSION: u32 = 4;
/// Words of the table.
pub const WORDS: usize = 21;

/// Word indexes of the table (byte offset = 4 * index; `hooks-dt2.s` and `hooks-dn2.s` name the byte offsets `T_*`, and a test compares them). The hooks leave through the
/// imports the port file resolves, so the table has no word for a firmware address.
pub mod w {
    pub const MAGIC: usize = 0;
    pub const VERSION: usize = 1;
    /// 1 Digitakt II, 2 Digitone II (`[digikit] id` of the device file).
    pub const DEVICE: usize = 2;
    /// The registry: `machines.first`, `.count`, `.entries`, `.rows`.
    pub const FIRST: usize = 3;
    pub const COUNT: usize = 4;
    pub const ENTRIES: usize = 5;
    pub const ROWS: usize = 6;
    /// `mf_ensure` of the image (builds the registry's run-time tables once).
    pub const ENSURE: usize = 7;
    /// The adapter's native page functions (`mf_native_*`) the hooks use.
    pub const N_PAGE_COUNT: usize = 8;
    pub const N_DESCRIPTOR: usize = 9;
    pub const N_CODE: usize = 10;
    pub const N_DRAW: usize = 11;
    pub const N_OVERLAY: usize = 12;
    /// Digitakt II: the table `mf_slotmap`.
    pub const SLOTMAP: usize = 13;
    /// Digitone II: the page rows `pages.rows` and `pages.count`.
    pub const PGROWS: usize = 14;
    pub const PGN: usize = 15;
    /// The adapter's native page calls of the registry (`mf_native_register`, `mf_native_register_v2`, `mf_native_remove`, `mf_native_turn`): the service calls them.
    pub const N_REGISTER: usize = 16;
    pub const N_REGISTER_V2: usize = 17;
    pub const N_REMOVE: usize = 18;
    pub const N_TURN: usize = 19;
    /// The adapter's native filter graph draw (`mf_native_filter_graph`).
    pub const N_FILTER_GRAPH: usize = 20;
}
