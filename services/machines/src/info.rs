//! The image table `machines_info` of `core/machines` (its `[[info]]`): what the machines service reads of the patched image. The owner copies the table into `DKRT_MF_TAB`
//! (`bind`, from `Provider::start`), where the assembler hooks of the service file read it.
//!
//! The words are big endian in the image (ColdFire is big endian, so a read of the word is the value). A word that is 0 was not bound: the service does not call it.
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkneeds::keys::name_hash;
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::{E_NO_REGISTRY, OK};

/// `MACH`.
pub const MAGIC: u32 = 0x4d41_4348;
/// Version 5: the native page functions left the table (the service has them, `native_page/`; version 4 had the adapter's register, remove and turn words and the hooks' five
/// page calls) and the image gives the copy of the stock type reader (`READER_REAL`) the page code needs. Version 4: the stock list order, the row size and the offset
/// of `position` (the service builds the machine list and the rank pairs).
pub const VERSION: u32 = 5;
/// Words of the table.
pub const WORDS: usize = 15;

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
    /// Digitakt II: the table `mf_slotmap`.
    pub const SLOTMAP: usize = 8;
    /// Digitone II: the page rows `pages.rows` and `pages.count`.
    pub const PGROWS: usize = 9;
    pub const PGN: usize = 10;
    /// The copy of the firmware's machine type reader (`mf_reader_real`), which gives the type the stock reader would (the hook of the reader maps the added types).
    /// 0 on a device without a hooked reader.
    pub const READER_REAL: usize = 11;
    /// The stock display order of the machine list: `first` words, one type each.
    pub const LIST_STOCK: usize = 12;
    /// Bytes of a registry row, and the offset of `position` in it.
    pub const ROW_SIZE: usize = 13;
    pub const O_POS: usize = 14;
}

/// The byte offsets the assembler files name (`.equ T_NAME, offset`), for the test that compares them with `w`.
pub const ASM_OFFSETS: &[(&str, usize)] = &[
    ("T_FIRST", w::FIRST),
    ("T_COUNT", w::COUNT),
    ("T_ROWS", w::ROWS),
    ("T_ENSURE", w::ENSURE),
    ("T_SLOTMAP", w::SLOTMAP),
    ("T_PGROWS", w::PGROWS),
    ("T_PGN", w::PGN),
];

/// The image table `machines_info`, copied by `bind`; the hooks of the service file read it (`T_*` of the assembler files). It is a table the assembler reads by name, so it stays
/// a named static of the crate (`README.md`).
#[no_mangle]
pub static mut DKRT_MF_TAB: [u32; WORDS] = [0; WORDS];

/// Bind the image table into `DKRT_MF_TAB`, or refuse (`E_NO_REGISTRY`: no usable `core/machines` in the image).
///
/// # Safety
/// Nothing reads `DKRT_MF_TAB` meanwhile (the hooks are declared after it).
pub unsafe fn bind<C: CoreApi>(core: &C) -> i32 {
    bind_table(core, addr_of_mut!(DKRT_MF_TAB) as *mut u32)
}

/// The table of the image, or null: the image has no `core/machines`.
unsafe fn image<C: CoreApi>(core: &C) -> *const u32 {
    core.info(name_hash("machines_info"))
}

/// Is `src` a table this service can use (magic, version, the device the core runs on, and the words the hooks cannot do without)?
unsafe fn usable<C: CoreApi>(core: &C, src: *const u32) -> bool {
    !src.is_null()
        && read_volatile(src.add(w::MAGIC)) == MAGIC
        && read_volatile(src.add(w::VERSION)) == VERSION
        && read_volatile(src.add(w::DEVICE)) == core.device_id()
        && read_volatile(src.add(w::ENSURE)) != 0
        && read_volatile(src.add(w::ROWS)) != 0
}

/// Copy the image table to `dst` (`WORDS` words), the magic last: a table is usable only when every word is in. `E_NO_REGISTRY` when the image has no usable table.
///
/// # Safety
/// `dst` points at `WORDS` words.
pub unsafe fn bind_table<C: CoreApi>(core: &C, dst: *mut u32) -> i32 {
    let src = image(core);
    if !usable(core, src) {
        return E_NO_REGISTRY;
    }
    let mut i = 1;
    while i < WORDS {
        write_volatile(dst.add(i), read_volatile(src.add(i)));
        i += 1;
    }
    write_volatile(dst, MAGIC);
    OK
}

/// Build the machine list and the rank pairs (`list`) from the bound table and the stock order in the image. `None` when the table has no stock list or the registry does not fit.
///
/// # Safety
/// `bind` has run: `DKRT_MF_TAB` holds the image table.
pub unsafe fn build_list<C: CoreApi>(core: &C) -> Option<usize> {
    let t = core::ptr::addr_of!(DKRT_MF_TAB) as *const u32;
    let word = |i: usize| read_volatile(t.add(i));
    crate::list::from_image(
        core,
        word(w::LIST_STOCK),
        word(w::FIRST),
        word(w::COUNT),
        word(w::ROWS),
        word(w::ROW_SIZE),
        word(w::O_POS),
    )
}
