//! The image table of `core/fxpages`: its word names, the bound copy and the firmware words.

use super::*;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_sdk::*;

/// `name_hash` (FNV-1a 32) of `fxpages_info`, the data table of `core/fxpages` in the image: the service reads it through `CoreApi::info`, so it has no link name.
pub const INFO_HASH: u32 = fnv1a(b"fxpages_info");

const fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// Words of the table (byte offsets / 4); the value indices are bytes from byte 80. Words 0 to `IMAGE_WORDS` - 1 are the image's `fxpages_info` (version 4: data only,
/// the indices and the device numbers; the words 10, 11 and 49 to 60 held firmware addresses in version 3 and are 0 in the image: the service names the firmware and
/// the port file resolves it; `bind` puts the two stock list words of the hooks' builder into 10 and 11); the words that point at state (the rows, the list, the counters)
/// and the three computed words are this service's (`bind_table`): the state lives in this file's bss, next to the hooks that use it.
pub(super) mod w {
    pub const MAGIC: usize = 0;
    pub const DEVICE: usize = 2;
    pub const ROWS: usize = 3;
    pub const ROW_SIZE: usize = 4;
    pub const FIRST: usize = 5;
    pub const PAGES: usize = 6;
    pub const LIST: usize = 7;
    pub const LIST_CAP: usize = 8;
    pub const COUNT: usize = 9;
    pub const STOCK: usize = 10;
    pub const STOCK_N: usize = 11;
    pub const P_FIRST: usize = 12;
    pub const ARRAY_OFF: usize = 14;
    pub const VALUES_N: usize = 15;
    pub const TPL_PERCENT: usize = 16;
    pub const TPL_BIPOLAR: usize = 17;
    pub const TPL_MIX: usize = 18;
    pub const KIT_PTR: usize = 19;
    pub const INDICES: usize = 80; // bytes
    pub const IDS: usize = 28; // byte 112: address of the effect IDs of the rows (halfwords)
    pub const DEFS: usize = 29; // the default raw values (halfwords, one for each value index)
    pub const MARK_INDEX: usize = 37; // byte 148: the array index of the mark, and its value (word 38)
    pub const MARK_VALUE: usize = 38;
    pub const VERSION: usize = 1;
    pub const LAST_LOAD: usize = 30;
    pub const NLOAD: usize = 31;
    pub const NSAVE: usize = 32;
    pub const LAST_SAVE: usize = 33;
    pub const LOAD_SRC: usize = 34;
    pub const SAVE_DST: usize = 35;
    pub const NINIT: usize = 36;
    pub const SEND_ROW: usize = 40;
    pub const SEND_COUNT: usize = 41;
    pub const NSOUND: usize = 48;
    // words 49 to 60 were the return addresses and the stock key list (version 3); 61 to 63 are three device numbers
    pub const TRACK_PAGE: usize = 61;
    pub const TAIL: usize = 62;
    pub const STORE_OFF: usize = 63;
    // computed by `bind_table`
    pub const ROWS_N: usize = 64; // effect rows with values in the kit (VALUES_N / 8)
    pub const ID_OFF: usize = 65; // byte offset of the first effect ID in the storage record (STORE_OFF + 2 * VALUES_N)
    pub const MARK_OFF: usize = 66; // byte offset of the mark in the array (2 * MARK_INDEX)
}

/// The byte offsets the assembler of the hooks (`fx.dks`, `hooks.rs`) names (`.equ T_NAME, offset`), for the test that compares them with `w`.
pub const ASM_OFFSETS: &[(&str, usize)] = &[
    ("T_FIRST", w::FIRST),
    ("T_SLOTS", w::PAGES),
    ("T_VIDX", w::INDICES / 4),
    ("T_MARKVAL", w::MARK_VALUE),
    ("T_TRACKPAGE", w::TRACK_PAGE),
    ("T_TAIL", w::TAIL),
    ("T_STORE_OFF", w::STORE_OFF),
    ("T_ROWS_N", w::ROWS_N),
    ("T_ID_OFF", w::ID_OFF),
    ("T_MARK_OFF", w::MARK_OFF),
];

/// The two firmware words of the hooks' file: the stock list of the FX view and the `moveq N,D1` of its builder (the byte at +1 is the stock count). They are the
/// `dk_fw_` words (`DKRT_FXP_FW`) of the file's assembler, which the port file resolves; a host build has no firmware and no words.
pub(crate) fn firmware() -> [u32; 2] {
    #[cfg(target_arch = "m68k")]
    {
        extern "C" {
            static DKRT_FXP_FW: [u32; 2];
        }
        // SAFETY: the words are data of the service file's assembler, linked with this crate
        unsafe { read_volatile(core::ptr::addr_of!(DKRT_FXP_FW)) }
    }
    #[cfg(not(target_arch = "m68k"))]
    {
        [0, 0]
    }
}

/// The table of the effect pages when it is bound, or null (`bind` was not called or refused).
pub(crate) fn bound() -> *const u32 {
    let tab = addr_of_mut!(DKRT_FXP_TAB) as *const u32;
    // SAFETY: `tab` is this file's static
    if unsafe { read_volatile(tab.add(w::MAGIC)) } == INFO_MAGIC {
        tab
    } else {
        core::ptr::null()
    }
}

/// The table of the effect pages as this service reads it, or null (no `core/fxpages` in the image, another device, an older table than the hooks need). The first
/// call copies the image table (`CoreApi::info`) into `DKRT_FXP_TAB` and fills the words that point at the state of the hooks; `Fx::start` makes that call (`bind`) before the
/// file's hooks are installed. A later call after `unbind` binds with the file's firmware words again (a host test that changes the fake image table).
pub(crate) unsafe fn info<C: CoreApi>(core: &C) -> *const u32 {
    let tab = bound();
    if !tab.is_null() {
        return tab;
    }
    bind_with(core, firmware())
}

/// Copy the image table into the bound table, with `fw` as the firmware words (null: refused).
unsafe fn bind_with<C: CoreApi>(core: &C, fw: [u32; 2]) -> *const u32 {
    let tab = addr_of_mut!(DKRT_FXP_TAB) as *mut u32;
    let src = core.info(INFO_HASH);
    if src.is_null() || !bind_table(core, src, tab, fw) {
        return core::ptr::null();
    }
    tab
}

/// Is the image table `src` one this service can use, and its words copied to `tab` with the state words filled in.
pub(super) unsafe fn bind_table<C: CoreApi>(
    core: &C,
    src: *const u32,
    tab: *mut u32,
    fw: [u32; 2],
) -> bool {
    if read_volatile(src.add(w::MAGIC)) != INFO_MAGIC
        || read_volatile(src.add(w::VERSION)) < INFO_VERSION
        || read_volatile(src.add(w::DEVICE)) != core.device_id()
    {
        return false;
    }
    let (cap, n_values) = (
        read_volatile(src.add(w::LIST_CAP)),
        read_volatile(src.add(w::VALUES_N)),
    );
    let slots = read_volatile(src.add(w::PAGES));
    // the state is sized for the most the build supports: a table that asks for more is refused
    if cap as usize > LIST_WORDS
        || slots as usize > MAX_FX
        || n_values as usize > DEFS_MAX
        || (n_values as usize) / 8 > IDS_MAX
    {
        return false;
    }
    let mut i = 0;
    while i < IMAGE_WORDS {
        write_volatile(tab.add(i), read_volatile(src.add(i)));
        i += 1;
    }
    let at = |p: usize| p as u32;
    write_volatile(tab.add(w::STOCK), fw[0]);
    write_volatile(tab.add(w::STOCK_N), fw[1]);
    let state = [
        (w::ROWS, addr_of_mut!(DKRT_FXP_ROWS) as usize),
        (w::LIST, addr_of_mut!(DKRT_FXP_LIST_BUF) as usize),
        (w::COUNT, addr_of_mut!(DKRT_FXP_COUNT) as usize),
        (w::IDS, addr_of_mut!(DKRT_FXP_IDS) as usize),
        (w::DEFS, addr_of_mut!(DKRT_FXP_DEFS) as usize),
        (w::LAST_LOAD, addr_of_mut!(DKRT_FXP_LAST_LOAD) as usize),
        (w::NLOAD, addr_of_mut!(DKRT_FXP_NLOAD) as usize),
        (w::NSAVE, addr_of_mut!(DKRT_FXP_NSAVE) as usize),
        (w::LAST_SAVE, addr_of_mut!(DKRT_FXP_LAST_SAVE) as usize),
        (w::LOAD_SRC, addr_of_mut!(DKRT_FXP_LOAD_SRC) as usize),
        (w::SAVE_DST, addr_of_mut!(DKRT_FXP_SAVE_DST) as usize),
        (w::NINIT, addr_of_mut!(DKRT_FXP_NINIT) as usize),
        (w::SEND_ROW, addr_of_mut!(DKRT_FXP_SEND_ROW) as usize),
        (w::SEND_COUNT, addr_of_mut!(DKRT_FXP_SEND_COUNT) as usize),
        (w::NSOUND, addr_of_mut!(DKRT_FXP_NSOUND) as usize),
    ];
    for (word, addr) in state {
        write_volatile(tab.add(word), at(addr));
    }
    write_volatile(tab.add(w::ROWS_N), n_values / 8);
    write_volatile(
        tab.add(w::ID_OFF),
        read_volatile(src.add(w::STORE_OFF)) + 2 * n_values,
    );
    write_volatile(
        tab.add(w::MARK_OFF),
        2 * read_volatile(src.add(w::MARK_INDEX)),
    );
    // the track FX key's list: the track FX page, then the send page (after the effect rows)
    write_volatile(
        addr_of_mut!(DKRT_FXP_KEY_LIST) as *mut u32,
        read_volatile(src.add(w::TRACK_PAGE)),
    );
    write_volatile(
        (addr_of_mut!(DKRT_FXP_KEY_LIST) as *mut u32).add(1),
        read_volatile(src.add(w::FIRST)) + slots,
    );
    // the magic last: a table is usable only when every word is in
    write_volatile(tab.add(w::MAGIC), INFO_MAGIC);
    true
}

/// What `Fx::start` does before the file's hooks are installed: the table is bound with the firmware words `fw`, or the file is refused (`E_NO_REGISTRY`: no usable
/// `core/fxpages` in the image).
pub unsafe fn bind<C: CoreApi>(core: &C, fw: [u32; 2]) -> i32 {
    // a second start (a reload of the file) keeps the live table: the image table is not copied over it
    if !bound().is_null() {
        return OK;
    }
    if bind_with(core, fw).is_null() {
        E_NO_REGISTRY
    } else {
        OK
    }
}

/// Forget the bound table: the next `info` reads the image table again (host tests that change the fake image table).
pub unsafe fn unbind() {
    write_volatile(addr_of_mut!(DKRT_FXP_TAB) as *mut u32, 0);
}

/// The kit's value array index of word `j` of effect row `k`.
#[inline]
pub(super) unsafe fn value_index(inf: *const u32, k: usize, j: usize) -> u32 {
    read_volatile((inf as *const u8).add(w::INDICES + k * WORDS + j)) as u32
}

/// Whether this image can show effect pages at all (`core/fxpages` and `core/rows` are in it).
pub unsafe fn available<C: CoreApi>(core: &C) -> bool {
    !info(core).is_null()
}
