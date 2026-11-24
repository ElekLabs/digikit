//! What the machines service takes of the image: the machine registry's words of `rows_info` (`core/rows`), bound into `DKRT_MF_TAB` by `bind`, where the assembler hooks of the service file read them.
//!
//! The words are big endian in the image (ColdFire is big endian, so a read of the word is the value). A word that is 0 was not bound: the service does not call it.
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::{E_NO_REGISTRY, OK};
use dkrt_svc_rows::rows_info::w as r;

/// `MACH`: the first word of the bound table, written last (a table is usable only when every word is in).
pub const MAGIC: u32 = 0x4d41_4348;
/// Version 6: the words come from `rows_info` of the image (version 5 was the image table `machines_info`, which had `mf_ensure`).
pub const VERSION: u32 = 6;
/// Words of the table.
pub const WORDS: usize = 17;

/// Word indexes of the bound table (byte offset = 4 * index; `hooks-dt2.s` and `hooks-dn2.s` name the byte offsets `T_*`, and a test compares them). The hooks leave
/// through the imports the port file resolves, so the table has no word for a firmware address.
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
    /// Digitakt II: the table `mf_slotmap`.
    pub const SLOTMAP: usize = 7;
    /// Digitone II: the page rows `pages.rows` and `pages.count`.
    pub const PGROWS: usize = 8;
    pub const PGN: usize = 9;
    /// The copy of the firmware's machine type reader (`mf_reader_real`), which gives the type the stock reader would (the hook of the reader maps the added types).
    /// 0 on a device without a hooked reader.
    pub const READER_REAL: usize = 10;
    /// The stock display order of the machine list: `first` words, one type each.
    pub const LIST_STOCK: usize = 11;
    /// Bytes of a registry row, and the offset of `position` in it.
    pub const ROW_SIZE: usize = 12;
    pub const O_POS: usize = 13;
    /// The first reserved (run-time) machine type and how many there are: the rows the list copy leaves out while they have no name.
    pub const DYN0: usize = 14;
    pub const DYN_N: usize = 15;
    /// The offset of the name pointer in a registry row.
    pub const O_NAME: usize = 16;
}

/// The byte offsets the assembler files name (`.equ T_NAME, offset`), for the test that compares them with `w`.
pub const ASM_OFFSETS: &[(&str, usize)] = &[
    ("T_FIRST", w::FIRST),
    ("T_COUNT", w::COUNT),
    ("T_ROWS", w::ROWS),
    ("T_SLOTMAP", w::SLOTMAP),
    ("T_PGROWS", w::PGROWS),
    ("T_PGN", w::PGN),
];

/// The bound table, copied by `bind`; the hooks of the service file read it (`T_*` of the assembler files). It is a table the assembler reads by name, so it stays
/// a named static of the crate (`README.md`).
#[no_mangle]
pub static mut DKRT_MF_TAB: [u32; WORDS] = [0; WORDS];

/// Bind the registry's words of the image into `DKRT_MF_TAB`, or refuse (`E_NO_REGISTRY`: no usable `core/rows` in the image).
///
/// # Safety
/// Nothing reads `DKRT_MF_TAB` meanwhile (the hooks are declared after it).
pub unsafe fn bind<C: CoreApi>(core: &C) -> i32 {
    bind_table(core, addr_of_mut!(DKRT_MF_TAB) as *mut u32)
}

/// Is `src` a table this service can use (`rows_info` of this device, with the machine registry and the stock list)?
unsafe fn usable<C: CoreApi>(core: &C, src: *const u32) -> bool {
    !src.is_null()
        && read_volatile(src.add(r::DEVICE)) == core.device_id()
        && read_volatile(src.add(r::M_ROWS)) != 0
}

/// Copy the registry's words to `dst` (`WORDS` words), the magic last. `E_NO_REGISTRY` when the image has no usable `rows_info`.
///
/// # Safety
/// `dst` points at `WORDS` words.
pub unsafe fn bind_table<C: CoreApi>(core: &C, dst: *mut u32) -> i32 {
    let src = dkrt_svc_rows::rows_info(core);
    if !usable(core, src) {
        return E_NO_REGISTRY;
    }
    let from = |i: usize| read_volatile(src.add(i));
    let words: [(usize, u32); WORDS - 1] = [
        (w::VERSION, VERSION),
        (w::DEVICE, from(r::DEVICE)),
        (w::FIRST, from(r::M_FIRST)),
        (w::COUNT, from(r::M_COUNT)),
        (w::ENTRIES, from(r::M_ENTRIES)),
        (w::ROWS, from(r::M_ROWS)),
        (w::SLOTMAP, from(r::SLOTMAP)),
        (w::PGROWS, from(r::PG_ROWS)),
        (w::PGN, from(r::PG_COUNT)),
        (w::READER_REAL, from(r::READER_REAL)),
        (w::LIST_STOCK, from(r::LIST_STOCK)),
        (w::ROW_SIZE, from(r::M_ROW_SIZE)),
        (w::O_POS, from(r::O_POS)),
        (w::DYN0, from(r::M_DYN0)),
        (w::DYN_N, from(r::M_DYN_N)),
        (w::O_NAME, from(r::O_NAME)),
    ];
    for (i, v) in words {
        write_volatile(dst.add(i), v);
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
