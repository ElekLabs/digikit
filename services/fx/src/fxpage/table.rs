//! The table of the effect pages: its word names, the bound copy (made from the device facts and `rows_info`) and the firmware words.

use super::*;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_sdk::*;

/// Words of the table (byte offsets / 4); the value indices are bytes from byte 80. Words 0 to `IMAGE_WORDS` - 1 are made from the device facts (`DeviceFacts`: the indices and
/// the device numbers), the service's own constants and the parameter codes of the blocks of `rows_info`; the words 10, 11 and 49 to 60 held firmware addresses once and are 0:
/// the service names the firmware and the port file resolves it (`bind` puts the two stock list words of the hooks' builder into 10 and 11); the words that point at state (the
/// rows, the list, the counters) and the three computed words are this service's (`bind_table`): the state lives in this file's bss, next to the hooks that use it.
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
    pub const KNOB_ROWS: usize = 13; // parameter rows for each effect row: 7 knobs and MIX
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
    pub const LAST_LOAD: usize = 30;
    pub const NLOAD: usize = 31;
    pub const NSAVE: usize = 32;
    pub const LAST_SAVE: usize = 33;
    pub const LOAD_SRC: usize = 34;
    pub const SAVE_DST: usize = 35;
    pub const NINIT: usize = 36;
    pub const SEND_CELLS: usize = 39;
    pub const SEND_ROW: usize = 40;
    pub const SEND_COUNT: usize = 41;
    pub const P_SEND: usize = 42; // the parameter code of send cell 0; cell c is + c
    pub const SEND_IDX: usize = 43; // the Sound array index of send cell 0 and 1
    pub const SEND_TEMPLATE: usize = 45;
    pub const SOUND_OFF: usize = 46;
    pub const SOUND_STRIDE: usize = 47;
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

/// The table of the effect pages as this service reads it, or null (no `rows_info` or no block of the effect pages in the image, no facts for this device, a build that holds
/// no state for them). The first call makes the table (`bind_with`) in `DKRT_FXP_TAB` and fills the words that point at the state of the hooks; `Fx::start` makes that call
/// (`bind`) before the file's hooks are installed. A later call after `unbind` binds again with the file's firmware words (a host test that changes the fake image).
pub(crate) unsafe fn info<C: CoreApi>(core: &C) -> *const u32 {
    let tab = bound();
    if !tab.is_null() {
        return tab;
    }
    bind_with(core, firmware())
}

/// Bind the facts of the core's device, with `fw` as the firmware words (null: refused).
unsafe fn bind_with<C: CoreApi>(core: &C, fw: [u32; 2]) -> *const u32 {
    match device_facts(core.device_id()) {
        Some(f) => bind_facts_table(core, f, fw),
        None => core::ptr::null(),
    }
}

unsafe fn bind_facts_table<C: CoreApi>(core: &C, f: &DeviceFacts, fw: [u32; 2]) -> *const u32 {
    let tab = addr_of_mut!(DKRT_FXP_TAB) as *mut u32;
    if bind_table(core, f, tab, fw) {
        tab
    } else {
        core::ptr::null()
    }
}

/// Can `f` be bound in this image (the parameter blocks of `rows_info` hold its rows), and its words put into `tab` with the state words filled in.
pub(super) unsafe fn bind_table<C: CoreApi>(
    core: &C,
    f: &DeviceFacts,
    tab: *mut u32,
    fw: [u32; 2],
) -> bool {
    // the codes of the parameter rows are the image's: the blocks `core/rows` reserved for the effect knobs and the send knobs
    let rows = dkrt_svc_rows::rows_info(core);
    if rows.is_null() {
        return false;
    }
    let (Some(knobs), Some(send)) = (
        dkrt_svc_rows::block::block(rows, f.knob_block),
        dkrt_svc_rows::block::block(rows, f.send_block),
    ) else {
        return false;
    };
    // the state is sized for the most the build supports: facts that ask for more are refused, and so is an image whose blocks hold fewer rows than the facts use
    let (n_values, slots) = (f.values_n as usize, f.slots as usize);
    if slots > MAX_FX
        || n_values > DEFS_MAX
        || n_values / 8 > IDS_MAX
        || n_values > f.value_indices.len()
        || knobs.rows < (slots * WORDS) as u32
        || send.rows < f.send_cells
    {
        return false;
    }
    let mut i = 0;
    while i < IMAGE_WORDS {
        write_volatile(tab.add(i), 0);
        i += 1;
    }
    let consts = [
        (w::DEVICE, f.device),
        (w::ROW_SIZE, ROW_BYTES as u32),
        (w::FIRST, f.page_first),
        (w::PAGES, f.slots),
        (w::LIST_CAP, LIST_WORDS as u32),
        (w::P_FIRST, knobs.first),
        (w::KNOB_ROWS, WORDS as u32),
        (w::ARRAY_OFF, f.array_off),
        (w::VALUES_N, f.values_n),
        (w::TPL_PERCENT, f.template_percent),
        (w::TPL_BIPOLAR, f.template_bipolar),
        (w::TPL_MIX, f.template_mix),
        (w::KIT_PTR, f.kit_ptr),
        (w::MARK_INDEX, f.mark_index),
        (w::MARK_VALUE, FXP_MARK),
        (w::SEND_CELLS, f.send_cells),
        (w::P_SEND, send.first),
        (w::SEND_IDX, f.send_index[0]),
        (w::SEND_IDX + 1, f.send_index[1]),
        (w::SEND_TEMPLATE, f.send_template),
        (w::SOUND_OFF, f.sound_off),
        (w::SOUND_STRIDE, f.sound_stride),
        (w::TRACK_PAGE, f.track_page),
        (w::TAIL, f.tail_off),
        (w::STORE_OFF, f.store_off),
        (w::STOCK, fw[0]),
        (w::STOCK_N, fw[1]),
    ];
    for (word, v) in consts {
        write_volatile(tab.add(word), v);
    }
    i = 0;
    while i < f.value_indices.len() {
        write_volatile((tab as *mut u8).add(w::INDICES + i), f.value_indices[i]);
        i += 1;
    }
    let at = |p: usize| p as u32;
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
    write_volatile(tab.add(w::ROWS_N), f.values_n / 8);
    write_volatile(tab.add(w::ID_OFF), f.store_off + 2 * f.values_n);
    write_volatile(tab.add(w::MARK_OFF), 2 * f.mark_index);
    // the track FX key's list: the track FX page, then the send page (after the effect rows)
    write_volatile(addr_of_mut!(DKRT_FXP_KEY_LIST) as *mut u32, f.track_page);
    write_volatile(
        (addr_of_mut!(DKRT_FXP_KEY_LIST) as *mut u32).add(1),
        f.page_first + f.slots,
    );
    // the magic last: a table is usable only when every word is in
    write_volatile(tab.add(w::MAGIC), INFO_MAGIC);
    true
}

/// What `Fx::start` does before the file's hooks are installed: the table is bound for the core's device with the firmware words `fw`, or the file is refused
/// (`E_NO_REGISTRY`: the image has no `rows_info` with the blocks of the effect pages, or this device has no effect pages).
pub unsafe fn bind<C: CoreApi>(core: &C, fw: [u32; 2]) -> i32 {
    // a second start (a reload of the file) keeps the live table: it is not made again over it
    if !bound().is_null() {
        return OK;
    }
    if bind_with(core, fw).is_null() {
        E_NO_REGISTRY
    } else {
        OK
    }
}

/// Bind `f` instead of the facts of the core's device (the seam of host tests: facts of a small device, another kit layout); the table is made again over a bound one.
pub unsafe fn bind_facts<C: CoreApi>(core: &C, f: &DeviceFacts, fw: [u32; 2]) -> i32 {
    if bind_facts_table(core, f, fw).is_null() {
        E_NO_REGISTRY
    } else {
        OK
    }
}

/// Forget the bound table: the next `info` makes it again (host tests that change the fake image).
pub unsafe fn unbind() {
    write_volatile(addr_of_mut!(DKRT_FXP_TAB) as *mut u32, 0);
}

/// The kit's value array index of word `j` of effect row `k`.
#[inline]
pub(super) unsafe fn value_index(inf: *const u32, k: usize, j: usize) -> u32 {
    read_volatile((inf as *const u8).add(w::INDICES + k * WORDS + j)) as u32
}

/// Whether this image can show effect pages at all (`core/rows` has the blocks and this device has the facts).
pub unsafe fn available<C: CoreApi>(core: &C) -> bool {
    !info(core).is_null()
}
