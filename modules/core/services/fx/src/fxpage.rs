//! Effect pages: the FX view page and the knobs of a run-time effect (docs/design/digikit-registry.md "Effect pages", docs/findings/61-fx-pages.md).
//!
//! A `.dkm` that carries an EFFECT record (`register` kind 4) for one of its effects gets a page after CHO in the FX view of the Digitakt II. The page is built
//! from rows that `core/fxpages` reserved at patch time and that nothing here names a device for: `fxpages_info` is the table of their addresses, the kit's value
//! array and the device's free value indices. For effect row k (the k-th free row of `core/dynfx`) the record makes
//!
//! - the page row k of `fxpages`: tab label, title and the nine parameter codes (the knobs at their positions, MIX on the level slot),
//! - eight parameter rows (`params.fp<8k + j>`): knob j (the effect's parameter word p_j) and MIX (j = 7), each pointing at the kit's value array index
//!   `value_indices[8k + j]` as its mirror index, so the firmware's own page code shows and edits the value there,
//! - the list of pages of the FX view (the stock three, then the pages of the loaded effects in row order).
//!
//! `tick` (once per UI-loop pass) reads the values of the loaded effects from the kit's array and sends a changed value to the effect's row in the DSP, one word
//! per transfer through window 2 of `core/dsp-link`: knob j as a float in [min, max] into p_j (min + (max - min) * raw / 0x7f00), MIX as a float 0 .. 1 into the row's
//! mix word (raw / 0x7f00), which the adapter `core/fx` applies as the dry/wet blend. A value is the raw s16 of the array, 0 to 0x7f00 (the display shows raw / 256).
//! The values are saved with the kit (persistence, finding 61): the hooks `fxp_load` and `fxp_save` of `core/fxpages` run in the firmware's kit loader and saver, put the
//! values of the loaded effects into the kit's spare storage halfwords and take them out again, together with the effect's ID (the EFFECT record's `id`). A load
//! gives an effect row the stored values only when the stored ID is the one of the effect now in the row, and the effect's defaults otherwise, before the UI loop
//! looks at the array again, so no zero ever reaches the DSP. A third hook, `fxp_init` (the firmware's initialiser of the array: a new kit, a fresh kit that Load Kit fills),
//! gives each loaded effect its defaults. Both put a mark in a spare index of the array; a kit object without the mark (another pattern's kit, made before the effect was loaded) gets the
//! defaults the first time `tick` finds it live, before any value is read from it.
//! The array is the live kit's (`[fx] kit_ptr` of the device file): the kit accessor of the firmware (`fw_kit_of`) answers another object, the project's.
//!
//! Everything runs in the UI task. No float arithmetic runs in an interrupt; the arithmetic is the compiler's soft float.
use crate::dsp as fxdsp;
use crate::fxsend;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::{cstr_len, iw, rd32};
use dkrt_sdk::*;

pub use dkrt_sdk::records::{KIND_EFFECT, LEN_EFFECT};
const INFO_MAGIC: u32 = 0x4658_5047; // "FXPG"
use dkrt_abi::dsp::MAX_FX;
const KNOBS: usize = 7;
const WORDS: usize = 8; // the knobs and MIX
const TEXT: usize = 24;
const REP_SIZE: usize = 48;
const KNOB_REC: usize = 52;
const KNOB_AT: usize = 48;
const RAW_MAX: i32 = 0x7f00;
const RAW_F: f32 = 32512.0; // 0x7f00

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
/// the port file resolves it; `set_firmware` puts the two stock list words of the hooks' builder into 10 and 11); the words that point at state (the rows, the list, the counters)
/// and the three computed words are this service's (`bind_table`): the state lives in this file's bss, next to the hooks that use it.
mod w {
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

/// The words of the image table that the service copies, and of its own table.
const IMAGE_WORDS: usize = 64;
const TAB_WORDS: usize = 72;
/// The version of the image table that has no firmware address in it (`w::VERSION`).
const INFO_VERSION: u32 = 4;
const ROW_BYTES: usize = 0x2c;
const LIST_WORDS: usize = 8;
const IDS_MAX: usize = 8;
const DEFS_MAX: usize = 64;

// ---- the state of the hooks: in this file's bss; the asm of the hooks (`fx.dks`) names these symbols, and `bind_table` puts their addresses into `DKRT_FXP_TAB` ----------
/// The table as this service reads it (`info`), and as the hooks read the constants they need.
#[no_mangle]
pub static mut DKRT_FXP_TAB: [u32; TAB_WORDS] = [0; TAB_WORDS];
/// The page rows of the effect pages (`ROW_BYTES` each: name, title, 9 codes), and the send page's row after them.
#[no_mangle]
pub static mut DKRT_FXP_ROWS: [u8; ROW_BYTES * MAX_FX] = [0; ROW_BYTES * MAX_FX];
/// The empty firmware string (`make_rep` of "": length 0, capacity 0, refcount -1, NUL) that the name and title words of a released page row point to.
pub static mut DKRT_FXP_EMPTY: Aligned16 = Aligned16([0; 16]);
/// Four-byte alignment for a buffer that `make_rep` writes in words.
#[repr(C, align(4))]
pub struct Aligned16(pub [u8; 16]);
#[no_mangle]
pub static mut DKRT_FXP_SEND_ROW: [u8; ROW_BYTES] = [0; ROW_BYTES];
/// The count cell of the track FX key's list (0: the stock list; 2: the send page follows).
#[no_mangle]
pub static mut DKRT_FXP_SEND_COUNT: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_KEY_LIST: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut DKRT_FXP_NSOUND: u32 = 0;
/// The list of page numbers the FX view gets, and its count cell (0: the stock list).
#[no_mangle]
pub static mut DKRT_FXP_LIST_BUF: [u32; LIST_WORDS] = [0; LIST_WORDS];
#[no_mangle]
pub static mut DKRT_FXP_COUNT: u32 = 0;
/// The effect ID of each row and the default raw values (halfwords), written when an effect is registered.
#[no_mangle]
pub static mut DKRT_FXP_IDS: [u16; IDS_MAX] = [0; IDS_MAX];
#[no_mangle]
pub static mut DKRT_FXP_DEFS: [u16; DEFS_MAX] = [0; DEFS_MAX];
/// Counters and pointers of the kit hooks, for tests and the debugger.
#[no_mangle]
pub static mut DKRT_FXP_LAST_LOAD: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_NLOAD: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_NSAVE: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_LAST_SAVE: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_LOAD_SRC: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_SAVE_DST: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_NINIT: u32 = 0;

/// The effect pages' state: who owns each page (module record + 1), the values in force, and the strings the rows point to.
#[repr(C, align(4))]
pub struct FxpState {
    pub magic: u32,
    /// The kit object whose array the values were last put in or read from (0: none seen yet).
    pub kit: u32,
    pub polls: u32,
    pub changes: u32,
    pub sent: u32,
    pub errors: u32,
    pub seeds: u32,
    pub owner: [u8; MAX_FX],
    /// Knobs of the page (the words p0 .. p(n-1)); MIX is word 7.
    pub nknob: [u8; MAX_FX],
    /// The raw value (0 .. 0x7f00, as the kit's array holds it) in force, the one handed to the link as the word's live value (`want`) and the one the DSP has
    /// taken (`dsp`; -1: none).
    pub cur: [[i32; WORDS]; MAX_FX],
    pub want: [[i32; WORDS]; MAX_FX],
    pub dsp: [[i32; WORDS]; MAX_FX],
    /// The record's default raw values (knob j, MIX): what a kit gets that holds no values of this effect.
    pub def: [[i32; WORDS]; MAX_FX],
    /// The effect ID of each row (the record's; 0: no effect), as `fxp_ids` of core/fxpages has it for the kit hooks.
    pub id: [u32; MAX_FX],
    /// The live kit pointer changed to a kit object that had the mark (a pattern change back to a kit we know).
    pub adopts: u32,
    /// The float range of knob j: its minimum and its span max - min (f32 bits).
    pub min: [[u32; KNOBS]; MAX_FX],
    pub span: [[u32; KNOBS]; MAX_FX],
    tab: [[u8; TEXT]; MAX_FX],
    title: [[u8; TEXT]; MAX_FX],
    name: [[u8; 32]; MAX_FX * WORDS],
    short: [[u8; 16]; MAX_FX * WORDS],
    rep_tab: [[u8; REP_SIZE]; MAX_FX],
    rep_title: [[u8; REP_SIZE]; MAX_FX],
    /// What `remove` found at a row, one stamp for each row: the effect ID that left (0: none), a hash of the module's name, the live kit object, and the count of kit loads and
    /// initialisations at that time. A load of the same effect of the same module into the row finds the user's values still in the kit's array and keeps them (`apply`),
    /// unless a kit was loaded or the live kit changed in between. A row another module's effect takes, or one that is removed again, has its own stamp.
    parked_id: [u32; MAX_FX],
    parked_mod: [u32; MAX_FX],
    parked_kit: [u32; MAX_FX],
    parked_hooks: [u32; MAX_FX],
}

#[no_mangle]
pub static mut DKRT_FXP: FxpState = unsafe { core::mem::zeroed() };

fn fs() -> &'static mut FxpState {
    unsafe { &mut *addr_of_mut!(DKRT_FXP) }
}

/// The two firmware words the service file gives (`set_firmware`): the stock list of the FX view and the `moveq N,D1` of its builder (the byte at +1 is the stock count).
static mut FW_WORDS: [u32; 2] = [0; 2];

/// The firmware addresses the hooks' file resolved (its `dk_fw_` words): the stock page list and the address of the instruction whose byte at +1 is the stock count. The file
/// calls it from its `setup`, before `info` binds the table.
pub unsafe fn set_firmware(stock: u32, stock_n: u32) {
    write_volatile(addr_of_mut!(FW_WORDS), [stock, stock_n]);
}

/// The table of the effect pages as this service reads it, or null (no `core/fxpages` in the image, another device, an older table than the hooks need). The first
/// call copies the image table (`CoreApi::info`) into `DKRT_FXP_TAB` and fills the words that point at the state of the hooks; the service file's `init` makes that call
/// before its hooks are installed (`setup`).
pub(crate) unsafe fn info<C: CoreApi>(core: &C) -> *const u32 {
    let tab = addr_of_mut!(DKRT_FXP_TAB) as *mut u32;
    if read_volatile(tab.add(w::MAGIC)) == INFO_MAGIC {
        return tab;
    }
    let src = core.info(INFO_HASH);
    if src.is_null() || !bind_table(core, src, tab) {
        return core::ptr::null();
    }
    tab
}

/// Is the image table `src` one this service can use, and its words copied to `tab` with the state words filled in.
unsafe fn bind_table<C: CoreApi>(core: &C, src: *const u32, tab: *mut u32) -> bool {
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
    let fw = read_volatile(addr_of_mut!(FW_WORDS));
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

/// The service file's `init` step before its hooks are installed: the table is bound, or the file is refused (`E_NO_REGISTRY`: no usable `core/fxpages` in the image).
pub unsafe fn setup<C: CoreApi>(core: &C) -> i32 {
    if info(core).is_null() {
        E_NO_REGISTRY
    } else {
        OK
    }
}

/// Forget the bound table: the next `info` reads the image table again (host tests that change the fake image table; a service file that is unloaded).
pub unsafe fn unbind() {
    write_volatile(addr_of_mut!(DKRT_FXP_TAB) as *mut u32, 0);
}

/// The kit's value array index of word `j` of effect row `k`.
#[inline]
unsafe fn value_index(inf: *const u32, k: usize, j: usize) -> u32 {
    read_volatile((inf as *const u8).add(w::INDICES + k * WORDS + j)) as u32
}

unsafe fn text_ok(p: *const u8, field: usize, max: usize) -> bool {
    let n = cstr_len(p, field);
    if n == 0 || n > max || n == field {
        return false;
    }
    let mut i = 0;
    while i < field {
        let c = *p.add(i);
        if i < n {
            if !(0x20..0x7f).contains(&c) {
                return false;
            }
        } else if c != 0 {
            return false;
        }
        i += 1;
    }
    true
}

unsafe fn copy_text(dst: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
}

/// Whether this image can show effect pages at all (`core/fxpages` and `core/dynreg` are in it).
pub unsafe fn available<C: CoreApi>(core: &C) -> bool {
    !info(core).is_null()
}

/// The fixed-size EFFECT record at `b` (its body), checked before anything is written; the registry's `fxpages` table must be there. The fields are the rule of `check_body`.
pub unsafe fn check_record<C: CoreApi>(core: &C, b: *const u8) -> i32 {
    let inf = info(core);
    if inf.is_null() {
        return E_NO_REGISTRY;
    }
    check_body(b)
}

/// The rule of one EFFECT record at `b` (its body, `LEN_EFFECT - 8` bytes): ranges, texts, knob positions, minimum below maximum with the default inside. Reads nothing but the
/// body (the page slots are checked by `apply`), so `Fx::validate_records` runs it on a built module.
pub unsafe fn check_body(b: *const u8) -> i32 {
    let (fx, mix, n) = (rd32(b), rd32(b.add(4)), rd32(b.add(8)) as usize);
    let id = rd32(b.add(12));
    if fx as usize >= MAX_FX || mix > 127 || n > KNOBS || id == 0 || id > 0xffff {
        return E_FORMAT;
    }
    if !text_ok(b.add(16), 8, 3) || !text_ok(b.add(24), TEXT, TEXT - 1) {
        return E_FORMAT;
    }
    let mut seen = 0u32;
    let mut j = 0;
    while j < KNOBS {
        let k = b.add(KNOB_AT + j * KNOB_REC);
        if j < n {
            let (pos, disp) = (rd32(k), rd32(k.add(4)));
            let (lo, hi, d) = (
                f32::from_bits(rd32(k.add(8))),
                f32::from_bits(rd32(k.add(12))),
                f32::from_bits(rd32(k.add(16))),
            );
            if pos >= 8 || seen & (1 << pos) != 0 || disp > 1 {
                return E_FORMAT;
            }
            seen |= 1 << pos;
            // finite, min < max, the default inside (the comparisons are false for a NaN)
            if !(lo < hi) || !(d >= lo && d <= hi) || (hi - lo) == f32::INFINITY {
                return E_FORMAT;
            }
            if !text_ok(k.add(20), TEXT, TEXT - 1) || !text_ok(k.add(20 + TEXT), 8, 6) {
                return E_FORMAT;
            }
        }
        j += 1;
    }
    OK
}

/// The raw value of a float default: the nearest of 0 .. 0x7f00 over [min, max].
fn raw_of(def: f32, lo: f32, hi: f32) -> i32 {
    let f = (def - lo) / (hi - lo) * RAW_F + 0.5;
    let n = f as i32;
    if n < 0 {
        0
    } else if n > RAW_MAX {
        RAW_MAX
    } else {
        n
    }
}

/// The float a raw value stands for: min + span * (raw / 0x7f00); the maximum is exact.
#[inline]
fn knob_value(min: u32, span: u32, raw: i32) -> u32 {
    (f32::from_bits(min) + f32::from_bits(span) * (raw as f32 / RAW_F)).to_bits()
}

/// MIX as a float: raw / 0x7f00 (0x7f00 is exactly 1.0, 0 exactly 0).
#[inline]
fn mix_value(raw: i32) -> u32 {
    (raw as f32 / RAW_F).to_bits()
}

unsafe fn rebuild_list<C: CoreApi>(core: &C, inf: *const u32) {
    let s = fs();
    let list = iw(inf, w::LIST);
    let stock = iw(inf, w::STOCK);
    // the stock count is the byte of `moveq N,D1` in the view builder
    let n_stock = core.rd8(iw(inf, w::STOCK_N) + 1);
    let cap = iw(inf, w::LIST_CAP);
    let mut n = 0;
    while n < n_stock && n < cap {
        core.wr(list + 4 * n, core.rd(stock + 4 * n));
        n += 1;
    }
    // the send effects first, then the master effects, each in row order (FX routing: the send effects are the ones the track FX key has a page for)
    let mut pass = 0;
    while pass < 2 {
        let mut k = 0;
        while k < iw(inf, w::PAGES) as usize && k < MAX_FX {
            if s.owner[k] != 0 && n < cap && fxsend::is_send(k) == (pass == 0) {
                core.wr(list + 4 * n, iw(inf, w::FIRST) + k as u32);
                n += 1;
            }
            k += 1;
        }
        pass += 1;
    }
    // no page of ours: the count 0 keeps the stock list (the hook in the builder)
    core.wr(iw(inf, w::COUNT), if n == n_stock { 0 } else { n });
}

/// Build the page list again (a route changed the order).
pub unsafe fn relist<C: CoreApi>(core: &C) {
    let inf = info(core);
    if !inf.is_null() {
        rebuild_list(core, inf);
    }
}

/// The module record + 1 that owns the page of effect row `k` (0: no page).
#[inline]
pub fn page_owner(k: usize) -> u8 {
    if k < MAX_FX {
        fs().owner[k]
    } else {
        0
    }
}

/// The tab label of the page of effect row `k` (up to 3 characters) into `out`.
#[inline]
pub fn tab_of(k: usize, out: &mut [u8; 8]) {
    if k < MAX_FX {
        let t = &fs().tab[k];
        let mut i = 0;
        while i < 8 && t[i] != 0 {
            out[i] = t[i];
            i += 1;
        }
    }
}

/// Fill the page, the parameter rows and the list for the EFFECT record at `b` of module `owner` (module record + 1). The effect's row in the chain has been
/// claimed by `dsp::reserve`; the page and parameter rows belong to that row's number.
pub unsafe fn apply<C: CoreApi>(core: &C, owner: u8, b: *const u8) -> i32 {
    let inf = info(core);
    if inf.is_null() {
        return E_NO_REGISTRY;
    }
    let s = fs();
    let Some(k) = fxdsp::fx_row_of(core, owner, rd32(b)) else {
        return E_FORMAT; // no such effect in the module's DSP section
    };
    if k >= iw(inf, w::PAGES) as usize
        || k >= MAX_FX
        || (iw(inf, w::VALUES_N) as usize) < (k + 1) * WORDS
    {
        return E_PAGE_FULL;
    }
    if s.owner[k] != 0 {
        return E_PAGE_FULL;
    }
    let n = rd32(b.add(8)) as usize;
    let first = iw(inf, w::P_FIRST) + (k * WORDS) as u32;
    let row = iw(inf, w::ROWS) + k as u32 * iw(inf, w::ROW_SIZE);
    // strings
    copy_text(s.tab[k].as_mut_ptr(), b.add(16), 8);
    copy_text(s.title[k].as_mut_ptr(), b.add(24), TEXT);
    let tab = dkrt_svc_params::make_rep(s.rep_tab[k].as_mut_ptr(), s.tab[k].as_ptr(), false);
    let title = dkrt_svc_params::make_rep(s.rep_title[k].as_mut_ptr(), s.title[k].as_ptr(), false);
    // the page's nine codes: 0 where no knob stands
    let mut codes = [0u32; 9];
    let mut j = 0;
    while j < WORDS {
        let wi = k * WORDS + j;
        let (code, tpl, def_n) = if j < n {
            let kr = b.add(KNOB_AT + j * KNOB_REC);
            let (lo, hi) = (
                f32::from_bits(rd32(kr.add(8))),
                f32::from_bits(rd32(kr.add(12))),
            );
            copy_text(s.name[wi].as_mut_ptr(), kr.add(20), TEXT);
            copy_text(s.short[wi].as_mut_ptr(), kr.add(20 + TEXT), 8);
            let pos = rd32(kr) as usize;
            let tpl = if rd32(kr.add(4)) == 1 {
                iw(inf, w::TPL_BIPOLAR)
            } else {
                iw(inf, w::TPL_PERCENT)
            };
            let d = raw_of(f32::from_bits(rd32(kr.add(16))), lo, hi);
            s.min[k][j] = lo.to_bits();
            s.span[k][j] = (hi - lo).to_bits();
            codes[pos] = first + j as u32;
            (first + j as u32, tpl, d)
        } else if j == WORDS - 1 {
            copy_text(s.name[wi].as_mut_ptr(), b"Mix\0".as_ptr(), 4);
            copy_text(s.short[wi].as_mut_ptr(), b"MIX\0".as_ptr(), 4);
            codes[8] = first + j as u32;
            (
                first + j as u32,
                iw(inf, w::TPL_MIX),
                rd32(b.add(4)) as i32 * 256,
            )
        } else {
            j += 1;
            continue; // a word without a knob
        };
        let idx = value_index(inf, k, j);
        write_knob_row(core, s, k, j, code, tpl, idx, def_n);
        j += 1;
    }
    s.nknob[k] = n as u8;
    let id = rd32(b.add(12));
    publish_row(core, inf, k, id);
    let live = kit(core, inf);
    // the effect that left this row comes back (a reload or a live load after an unload), and the live kit is the one it left, with no kit loaded or initialised since:
    // the row's values in the array are the user's, so they stay and the next tick sends them
    let back = live != 0
        && id != 0
        && s.parked_id[k] == id
        && s.parked_mod[k] == fxsend::name_hash(core.module_name(owner))
        && s.parked_kit[k] == live
        && s.parked_hooks[k] == hook_runs()
        && core.rd16(live + iw(inf, w::ARRAY_OFF) + 2 * iw(inf, w::MARK_INDEX))
            == iw(inf, w::MARK_VALUE) as i32;
    s.parked_id[k] = 0;
    s.parked_mod[k] = 0;
    if live != 0 && !back {
        // a kit object is live already and holds nothing of this effect (the init hook ran before the effect was here): its defaults now
        seed_row(core, inf, k, live + iw(inf, w::ARRAY_OFF));
        mark(core, inf, live + iw(inf, w::ARRAY_OFF));
        s.seeds += 1;
    }
    fxdsp::fx_set_mix(core, k, mix_value(rd32(b.add(4)) as i32 * 256));
    // the page row: codes, then the strings; the owner and the list publish it
    let mut c = 0;
    while c < 9 {
        core.wr(row + 8 + 4 * c as u32, codes[c]);
        c += 1;
    }
    core.wr(row, tab);
    core.wr(row + 4, title);
    s.owner[k] = owner;
    rebuild_list(core, inf);
    fxsend::apply(core, k, owner, s.tab[k].as_ptr());
    OK
}

/// Give back the pages of module `owner`: out of the list first, then the rows back to the reserved state.
pub unsafe fn remove<C: CoreApi>(core: &C, owner: u8) {
    let inf = info(core);
    if inf.is_null() {
        return;
    }
    let s = fs();
    let mut gone = 0u32;
    let mut k = 0;
    while k < MAX_FX {
        if s.owner[k] == owner {
            s.owner[k] = 0;
            s.parked_id[k] = s.id[k];
            s.parked_mod[k] = fxsend::module_hash(k);
            s.parked_kit[k] = kit(core, inf);
            s.parked_hooks[k] = hook_runs();
            gone |= 1 << k;
        }
        k += 1;
    }
    if gone == 0 {
        return;
    }
    rebuild_list(core, inf);
    k = 0;
    while k < MAX_FX {
        if gone & (1 << k) != 0 {
            let row = iw(inf, w::ROWS) + k as u32 * iw(inf, w::ROW_SIZE);
            let mut c = 0;
            while c < 9 {
                core.wr(row + 8 + 4 * c as u32, 0);
                c += 1;
            }
            // name and title: the empty string, so no word of the row points to the state of the effect that left
            let empty = dkrt_svc_params::make_rep(
                addr_of_mut!(DKRT_FXP_EMPTY) as *mut u8,
                b"\0".as_ptr(),
                false,
            );
            core.wr(row, empty);
            core.wr(row + 4, empty);
            // the defaults go to 0 before the row's table is published: the kit hooks must not keep the defaults of the effect that left
            let mut j = 0;
            while j < WORDS {
                s.def[k][j] = 0;
                j += 1;
            }
            publish_row(core, inf, k, 0);
            j = 0;
            while j < WORDS {
                dkrt_svc_params::reset_param_row(
                    core,
                    dkrt_svc_params::dynreg(core),
                    iw(inf, w::P_FIRST) + (k * WORDS + j) as u32,
                );
                s.cur[k][j] = 0;
                s.def[k][j] = 0;
                s.dsp[k][j] = -1;
                s.want[k][j] = -1;
                j += 1;
            }
            s.nknob[k] = 0;
            fxdsp::fx_forget(core, k);
            fxsend::remove(core, k);
        }
        k += 1;
    }
}

/// Hand the kit hooks of `core/fxpages` the effect ID of row `k` (0: none) and its defaults. An effect's defaults go first and its ID after them, so a kit
/// load between the two stores cannot restore values against defaults that are not there yet; a row's ID goes to 0 first for the same reason.
unsafe fn publish_row<C: CoreApi>(core: &C, inf: *const u32, k: usize, id: u32) {
    let s = fs();
    let (ids, defs) = (iw(inf, w::IDS), iw(inf, w::DEFS));
    if id == 0 {
        core.wr_half(ids + 2 * k as u32, 0);
    }
    let mut j = 0;
    while j < WORDS {
        core.wr_half(defs + 2 * (k * WORDS + j) as u32, s.def[k][j] as u32);
        j += 1;
    }
    if id != 0 {
        core.wr_half(ids + 2 * k as u32, id);
    }
    s.id[k] = id;
}

/// Kit loads and kit initialisations the hooks of `core/fxpages` have done (their counters move when a kit's rows are replaced).
#[inline]
fn hook_runs() -> u32 {
    unsafe {
        read_volatile(addr_of_mut!(DKRT_FXP_NLOAD))
            .wrapping_add(read_volatile(addr_of_mut!(DKRT_FXP_NINIT)))
    }
}

/// Put the mark of `core/fxpages` into the kit array `arr`: its effect rows hold values.
#[inline]
unsafe fn mark<C: CoreApi>(core: &C, inf: *const u32, arr: u32) {
    core.wr16(
        arr + 2 * iw(inf, w::MARK_INDEX),
        iw(inf, w::MARK_VALUE) as i32,
    );
}

/// The defaults of effect row `k` into the kit array `arr`.
unsafe fn seed_row<C: CoreApi>(core: &C, inf: *const u32, k: usize, arr: u32) {
    let s = fs();
    let mut j = 0;
    while j < WORDS {
        if has_word(s, k, j) {
            core.wr16(arr + 2 * value_index(inf, k, j), s.def[k][j]);
            s.cur[k][j] = s.def[k][j];
        }
        j += 1;
    }
}

/// Is `a` in the cached DDR window of the device the runtime runs on? The window is a device fact: the profile of the device id (the generated table). A device
/// without a profile has no window.
fn in_ddr<C: CoreApi>(core: &C, a: u32) -> bool {
    dkrt_abi::platform::DeviceProfile::by_id(core.device_id()).is_some_and(|p| p.ddr_contains(a))
}

/// The live kit object (0 while the firmware has none); a pointer in a DDR cell, checked to lie in DDR. Any alignment: the Digitone II's kit objects are allocated at odd
/// addresses (its sound arrays have an odd stride), so the array's halfwords are read and written a byte at a time.
pub(crate) unsafe fn kit<C: CoreApi>(core: &C, inf: *const u32) -> u32 {
    let k = core.rd(iw(inf, w::KIT_PTR));
    if !in_ddr(core, k) {
        return 0;
    }
    k
}

/// Words of effect row `k` that exist: its knobs and MIX.
#[inline(never)]
fn has_word(s: &FxpState, k: usize, j: usize) -> bool {
    j == WORDS - 1 || j < s.nknob[k] as usize
}

/// Once per UI-loop pass: note which values the DSP has taken, put the values in a new kit's array, and hand the changed values to the link; then the send effects' amounts.
pub unsafe fn tick<C: CoreApi>(core: &C) {
    let s = fs();
    let mut any = false;
    let mut k = 0;
    while k < MAX_FX {
        any |= s.owner[k] != 0;
        k += 1;
    }
    if !any {
        return;
    }
    let inf = info(core);
    if inf.is_null() {
        return;
    }
    confirm(core);
    values(core, inf);
    fxsend::tick(core);
}

/// The effect pages' values: the kit's array against the DSP rows.
unsafe fn values<C: CoreApi>(core: &C, inf: *const u32) {
    let s = fs();
    let mut k;
    let kit = kit(core, inf);
    if kit == 0 {
        return;
    }
    let arr = kit + iw(inf, w::ARRAY_OFF);
    if core.rd16(arr + 2 * iw(inf, w::MARK_INDEX)) != iw(inf, w::MARK_VALUE) as i32 {
        // no mark: the hooks of core/fxpages have not given this kit object its rows (a kit that was there before the effect was loaded: another pattern's, or one the
        // firmware cleared without them): the defaults, and the mark, before anything is read from it (a zero is never sent)
        k = 0;
        while k < MAX_FX {
            if s.owner[k] != 0 {
                seed_row(core, inf, k, arr);
            }
            k += 1;
        }
        mark(core, inf, arr);
        s.seeds += 1;
        s.kit = kit;
        return;
    }
    if kit != s.kit {
        s.adopts += 1; // a kit object with its rows: the live kit changed (a pattern change); the poll sends what differs from the DSP
        s.kit = kit;
        return;
    }
    s.polls = s.polls.wrapping_add(1);
    k = 0;
    while k < MAX_FX {
        if s.owner[k] != 0 {
            let mut j = 0;
            while j < WORDS {
                if has_word(s, k, j) {
                    let raw = core.rd16(arr + 2 * value_index(inf, k, j));
                    let n = if raw < 0 {
                        0
                    } else if raw > RAW_MAX {
                        RAW_MAX
                    } else {
                        raw
                    };
                    if n != s.cur[k][j] {
                        s.cur[k][j] = n;
                        s.changes += 1;
                    }
                    // only when the row is in the DSP and no load is going on; the link sends the newest value of the word when it is free
                    if n != s.want[k][j] && fxdsp::fx_live(core, k) {
                        set_knob(core, s, k, j, n);
                    }
                }
                j += 1;
            }
        }
        k += 1;
    }
}

/// The parameter row of word `j` of effect row `k` (the code is the row's), and the record's default as the value in force, in the kit and in the DSP row.
unsafe fn write_knob_row<C: CoreApi>(
    c: &C,
    s: &mut FxpState,
    k: usize,
    j: usize,
    code: u32,
    tpl: u32,
    idx: u32,
    def_n: i32,
) {
    let wi = k * WORDS + j;
    dkrt_svc_params::write_fx_param_row(
        c,
        dkrt_svc_params::dynreg(c),
        code,
        tpl,
        idx,
        0,
        RAW_MAX as u32,
        def_n as u32,
        s.name[wi].as_ptr() as u32,
        s.short[wi].as_ptr() as u32,
    );
    s.cur[k][j] = def_n;
    s.def[k][j] = def_n;
    s.dsp[k][j] = def_n; // the DSP row gets the record's defaults (p words) and the mix below
    s.want[k][j] = def_n;
}

/// The link slot and the DSP word of word `j` of effect row `k` for the raw value `n`: knob j is word 4 + j (a float in [min, max]), MIX is word 13.
unsafe fn knob_word<C: CoreApi>(
    c: &C,
    s: &FxpState,
    k: usize,
    j: usize,
    n: i32,
) -> Option<(dkrt_abi::link::Slot, u32)> {
    let (word, bits) = if j == WORDS - 1 {
        (dkrt_abi::dsp::FXW_MIX, mix_value(n))
    } else {
        (4 + j as u32, knob_value(s.min[k][j], s.span[k][j], n))
    };
    fxdsp::fx_slot(c, k, word).map(|slot| (slot, bits))
}

/// Hand the raw value `n` of word `j` of effect row `k` to the link as the word's live value.
unsafe fn set_knob<C: CoreApi>(c: &C, s: &mut FxpState, k: usize, j: usize, n: i32) -> bool {
    match knob_word(c, s, k, j, n) {
        Some((slot, bits)) => {
            c.link_set(slot, bits);
            s.want[k][j] = n;
            true
        }
        None => false,
    }
}

/// Note the values the DSP has taken: a word whose live value the link confirms is the DSP's.
unsafe fn confirm<C: CoreApi>(core: &C) {
    let s = fs();
    let mut k = 0;
    while k < MAX_FX {
        if s.owner[k] != 0 {
            let mut j = 0;
            while j < WORDS {
                let n = s.want[k][j];
                if has_word(s, k, j) && n >= 0 && n != s.dsp[k][j] {
                    if let Some((slot, bits)) = knob_word(core, s, k, j, n) {
                        if core.link_confirmed(slot) == Some(bits) {
                            s.dsp[k][j] = n;
                            s.sent += 1;
                        }
                    }
                }
                j += 1;
            }
        }
        k += 1;
    }
    s.errors = core.link_errors();
}

/// Pages in use and the pages the image reserved (for CAPACITY-like reports and tests): (reserved, used).
pub unsafe fn counts<C: CoreApi>(core: &C) -> (u32, u32) {
    let inf = info(core);
    if inf.is_null() {
        return (0, 0);
    }
    let s = fs();
    let mut used = 0;
    let mut k = 0;
    while k < MAX_FX {
        if s.owner[k] != 0 {
            used += 1;
        }
        k += 1;
    }
    (iw(inf, w::PAGES), used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dkrt_test_support::{Call, FakeCore};

    /// A knob of effect row 2 is declared with a default, then its value changes: the parameter row is written first, then the new value is the word's live value.
    #[test]
    fn a_knob_gets_its_row_then_a_change_is_set_on_the_link() {
        let mut s: std::boxed::Box<FxpState> = unsafe { std::boxed::Box::new(core::mem::zeroed()) };
        let mut core = FakeCore::new();
        core.set_dynfx(0, 0);
        // the parameter table of the firmware, as a fake: rows of 0x40 bytes at 0x1000, the mirror index in the second word
        let mut t = std::vec![0; 96];
        t[dkrt_svc_params::dynreg::w::P_TABLE] = 0x1000;
        t[dkrt_svc_params::dynreg::w::PF_SIZE] = 0x40;
        core.set_dynreg(t);
        unsafe {
            s.min[2][1] = 0f32.to_bits();
            s.span[2][1] = 1f32.to_bits();
            write_knob_row(&core, &mut s, 2, 1, 120, 7, 33, 0x4000);
            assert_eq!(
                (s.cur[2][1], s.def[2][1], s.dsp[2][1], s.want[2][1]),
                (0x4000, 0x4000, 0x4000, 0x4000)
            );
            assert!(set_knob(&core, &mut s, 2, 1, 0x7f00));
            assert_eq!(s.want[2][1], 0x7f00);
            // a knob is word 4 + j of its row, MIX word 13, whatever its place among the knobs; the rows are 24 words apart
            set_knob(&core, &mut s, 3, WORDS - 1, 0x7f00);
        }
        assert_eq!(
            unsafe { dkrt_abi::mem::FwMem::rd(&core, 0x1000 + 120 * 0x40 + 4) },
            33,
            "the effect knob's row has the kit's value index as its mirror index"
        );
        let calls: std::vec::Vec<Call> = core
            .calls()
            .into_iter()
            .filter(|c| !matches!(c, Call::Wr(..)))
            .collect();
        assert!(matches!(
            calls[0],
            Call::Set {
                slot: dkrt_abi::link::Slot { win: 2, word: 53 },
                ..
            }
        ));
        assert!(matches!(
            calls[1],
            Call::Set {
                slot: dkrt_abi::link::Slot { win: 2, word: 85 },
                ..
            }
        ));
    }

    /// The offsets that the device tests read from `DKRT_FXP` (`tests/regression/runtime/fx-pages-ui.ts`).
    #[test]
    fn the_layout_the_device_tests_read_is_unchanged() {
        assert_eq!(core::mem::offset_of!(FxpState, sent), 16);
        assert_eq!(core::mem::offset_of!(FxpState, errors), 20);
        assert_eq!(core::mem::offset_of!(FxpState, seeds), 24);
        assert_eq!(core::mem::offset_of!(FxpState, owner), 28);
        assert_eq!(core::mem::offset_of!(FxpState, cur), 36);
        assert_eq!(core::mem::offset_of!(FxpState, dsp), 292);
    }

    /// Several turns before the link is free end as one write of the newest value, and the word counts as the DSP's when the link confirms it.
    #[test]
    fn a_fast_turn_sends_the_last_value_and_the_dsp_value_follows_the_confirmation() {
        let mut s: std::boxed::Box<FxpState> = unsafe { std::boxed::Box::new(core::mem::zeroed()) };
        let mut core = FakeCore::new();
        core.set_dynfx(0, 0);
        unsafe {
            s.min[2][1] = 0f32.to_bits();
            s.span[2][1] = 1f32.to_bits();
            write_knob_row(&core, &mut s, 2, 1, 120, 7, 33, 0x4000);
            for n in [0x100, 0x200, 0x300] {
                set_knob(&core, &mut s, 2, 1, n);
            }
            for _ in 0..50 {
                core.link.borrow_mut().pump();
            }
            let slot = fxdsp::fx_slot(&core, 2, 5).unwrap();
            let wire = core.link.borrow().t.words();
            assert_eq!(wire.len(), 1);
            assert_eq!(
                core.link_confirmed(slot),
                Some(knob_value(s.min[2][1], s.span[2][1], 0x300))
            );
        }
    }
}
