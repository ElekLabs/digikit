//! The service contract suite (`dkrt_test_support::contract`) for the machines service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/sine` (`src/module.c`: one machine with a page of one parameter) and a second machine with another key: each makes the builder calls through
//! the service's `build::*`. The image tables are those of the device profiles (`Profile` of `tests/build.rs`) as `core/dynreg` writes them, over the fake core's memory.
use core::ptr::addr_of;
use dkneeds::keys::name_hash;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{FREE_TYPE, I_VFIRST, I_VFREE, ROW_WORDS, WIN_VOICES};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::FwMem;
use dkrt_abi::profiles::{DN2, DT2};
use dkrt_abi::provider::Provider;
use dkrt_sdk::limits;
use dkrt_sdk::records::KINDS_MACHINES;
use dkrt_svc_machines::dsp::dyndsp;
use dkrt_svc_machines::machines::Machines;
use dkrt_svc_machines::records::{mach_owner, types_of};
use dkrt_svc_machines::rows::{self, vrows, VoiceRows};
use dkrt_svc_rows::dynreg::{w, RegState};
use dkrt_test_support::contract::{Dsp, Fixture, Init, Late, Lifecycle, Module, Rows};
use dkrt_test_support::{
    provider_contract, subject, Driver, FakeCore, Provided, Recipe, RowBuilder,
};

// the core imports of the file's code (no runtime is linked)
#[path = "../../../runtime/core/test-support/stubs/core_imports.rs"]
mod stubs;

// the service file's source: its `service!` call is the one place that lists the parts
#[path = "../dks/src/lib.rs"]
pub mod file;
pub use file::{builder, fw, DK_SVC_OWNER};

/// The registry state of the owner the suite's driver started: the owners of the parameter and page rows, and the strings the firmware's rows point at. The owner lives as long as the
/// `Lifecycle` of the test that calls this, and nothing changes it while the test reads.
fn reg() -> &'static RegState {
    // SAFETY: the instance is the driver's, live while the test's `Lifecycle` is; the test is the only caller of the owner then
    subject::<Machines, _>(|m| unsafe { &*(m.reg() as *const RegState) })
}

/// A pointer into the owner as a snapshot holds it (`OWNER_PTR_TAG | offset in the owner`): the words of the firmware's rows that point at strings of the owner.
fn tagged(address: usize) -> u32 {
    let base = subject::<Machines, _>(|m| m as *mut Machines as usize);
    dkrt_test_support::contract::OWNER_PTR_TAG | (address - base) as u32
}

/// The parts the file adopts, read from the row its `service!` call declares.
struct MachinesRecipe;

impl Recipe for MachinesRecipe {
    type P = Machines;
    const NAME: &'static str = "machines";
    fn parts(b: RowBuilder<Machines>) -> RowBuilder<Machines> {
        use dkrt_abi::provider::{
            check, PartLookup, DESCRIBE_NEWEST, DSP_NEWEST, PART_DESCRIBE, PART_DSP, PART_QUERY,
            PART_RECORDS, PART_TICK, QUERY_NEWEST, RECORDS_NEWEST, TICK_NEWEST,
        };
        // SAFETY: the row is a static of this binary
        let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
        let has = |tag, newest| matches!(row.part(tag, newest), PartLookup::Found { .. });
        assert!(
            has(PART_RECORDS, RECORDS_NEWEST)
                && has(PART_DSP, DSP_NEWEST)
                && has(PART_TICK, TICK_NEWEST)
                && has(PART_DESCRIBE, DESCRIBE_NEWEST)
                && !has(PART_QUERY, QUERY_NEWEST),
            "the file adopts the four parts this recipe builds, and no Query"
        );
        b.records(&KINDS_MACHINES).dsp().tick().describe()
    }
}

/// The builder calls of a module's `init`, on the owner that the suite's driver started.
mod build {
    use super::*;

    pub unsafe fn add_machine(
        core: &FakeCore,
        ctx: *mut u8,
        index: u32,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        subject::<Machines, _>(|m| m.builder().add_machine(core, ctx, index, name, short))
    }

    pub unsafe fn machine_add_page(
        core: &FakeCore,
        mach: *mut u8,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        subject::<Machines, _>(|m| m.builder().machine_add_page(core, mach, name, short))
    }

    pub unsafe fn page_add_param(
        core: &FakeCore,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        def: u32,
    ) -> *mut u8 {
        subject::<Machines, _>(|m| m.builder().page_add_param(core, p, name, short, def))
    }

    pub unsafe fn param_range(core: &FakeCore, prm: *mut u8, min: u32, max: u32) {
        subject::<Machines, _>(|m| m.builder().param_range(core, prm, min, max))
    }

    pub unsafe fn page_canvas(
        core: &FakeCore,
        p: *mut u8,
        flags: u32,
        draw: u32,
        event: u32,
        arg: u32,
    ) {
        subject::<Machines, _>(|m| m.builder().page_canvas(core, p, flags, draw, event, arg))
    }

    pub unsafe fn page_bind(core: &FakeCore, p: *mut u8, prm: *mut u8, slot: u32) {
        subject::<Machines, _>(|m| m.builder().page_bind(core, p, prm, slot))
    }
}

/// The first run-time machine type (the same on both devices of the suite).
const DYN_TYPE_FIRST: u32 = DT2.dyn_type_first;

/// The machine keys of the keys tables of the two modules (`hash24 << 8`).
const KEY_A: u32 = 0x00a1_0100;
const KEY_B: u32 = 0x00b2_0200;
/// The key of the module that is refused while its records apply.
const KEY_L1: u32 = 0x00c3_0300;

fn c(s: &'static [u8]) -> *const u8 {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr()
}

/// `tests/fixtures/modules/sine/src/module.c`: the machine `sine` (index 0), its page and the parameter Level (default 127).
fn sine(i: &mut Init) {
    unsafe {
        let m = build::add_machine(i.core, i.ctx(), 0, c(b"Sine\0"), c(b"SIN\0"));
        let p = build::machine_add_page(i.core, m, c(b"Sine\0"), c(b"SINE\0"));
        build::page_add_param(i.core, p, c(b"Level\0"), c(b"LEVEL\0"), 127);
    }
}

/// A second machine with two parameters, one with its own range: what `sine` is for another module key.
fn sine_two(i: &mut Init) {
    unsafe {
        let m = build::add_machine(i.core, i.ctx(), 0, c(b"Sine Two\0"), c(b"SN2\0"));
        let p = build::machine_add_page(i.core, m, c(b"Sine Two\0"), c(b"SINE2\0"));
        build::page_add_param(i.core, p, c(b"Level\0"), c(b"LEVEL\0"), 100);
        let t = build::page_add_param(i.core, p, c(b"Tone\0"), c(b"TONE\0"), 10);
        build::param_range(i.core, t, 0, 100);
    }
}

/// The machine of `sine` for another module key set: the key of `sine`, the same rows (one page, one parameter), other names, default and range.
fn sine_alt(i: &mut Init) {
    unsafe {
        let m = build::add_machine(i.core, i.ctx(), 0, c(b"Sine Alt\0"), c(b"ALT\0"));
        let p = build::machine_add_page(i.core, m, c(b"Sine Alt\0"), c(b"ALTP\0"));
        let l = build::page_add_param(i.core, p, c(b"Depth\0"), c(b"DEPTH\0"), 33);
        build::param_range(i.core, l, 5, 90);
    }
}

/// A machine with a canvas page: its machine row, page and parameter rows are written, then the registration of the native page in the firmware fails (the call
/// of the image answers an error), which `apply_record` reports after `apply` wrote the rows.
fn native_page_fails(i: &mut Init) {
    native_page(i);
    i.core.fw_result.set((-5i32) as usize);
}

/// The same machine, whose native page registers.
fn native_page_ok(i: &mut Init) {
    native_page(i);
}

fn native_page(i: &mut Init) {
    unsafe {
        let m = build::add_machine(i.core, i.ctx(), 0, c(b"Late\0"), c(b"LAT\0"));
        let knobs = build::machine_add_page(i.core, m, c(b"LATE\0"), c(b"L\0"));
        let start = build::page_add_param(i.core, knobs, c(b"Start\0"), c(b"START\0"), 0);
        let ctl = build::machine_add_page(i.core, m, c(b"LATE\0"), c(b"L\0"));
        build::page_canvas(
            i.core,
            ctl,
            (i.core.device == DN2.id) as u32,
            0x4100,
            0x4200,
            0x88,
        );
        build::page_bind(i.core, ctl, start, 0);
        // the Digitone II's native pages come in pairs (the second is the wave page); the Digitakt II has one page, without flags (record version 1)
        if i.core.device == DN2.id {
            let wave = build::machine_add_page(i.core, m, c(b"LATE WAVE\0"), c(b"L\0"));
            build::page_canvas(i.core, wave, 2, 0x4300, 0x4400, 0x99);
            build::page_bind(i.core, wave, start, 4);
        }
    }
}

/// The type word of each of the four voice rows of the image (`image` gives `dyndsp_info` first row 2): the word that frees a row.
const ENABLE: [(u32, u32, u32); 4] = [
    (WIN_VOICES, ROW_WORDS * 2, FREE_TYPE),
    (WIN_VOICES, ROW_WORDS * 3, FREE_TYPE),
    (WIN_VOICES, ROW_WORDS * 4, FREE_TYPE),
    (WIN_VOICES, ROW_WORDS * 5, FREE_TYPE),
];

/// The words that differ from the start after a release, with the value each may hold (`remove` gives a row back as a gap row, which the start state of this
/// image does not hold; any other value, such as the one the module wrote, is a leftover). The offsets are those of `image`:
/// - machine row (`0x30_0000`, 0x100 bytes): the name (0) is the empty firmware string; the long and short text (4, 8) are the representations of the row's MISSING
///   texts (`rep_long`, `rep_short` of the registry state: an absent module reads MISSING while the sticky map holds its number, E5d); the short name (0x4c) is the
///   row's own MISSING short text (`s.mach_short`); own_dsp (0x48) is 1 (the DSP gets the row's own number, which no engine has); word 0x30 after the eight knob codes (`O_F0 + 4 * KNOBS`) is the track level code 10; the position in the list (0x44) is not written by
///   `remove` and is no order in a hidden row (any value);
/// - names table (`0x40_0000`, 12 bytes per type), the types of the module rows: tab and title are the row's MISSING texts (`s.mach_name`, `s.mach_short`);
/// - parameter registry row (`0x20_0000`, 0x20 bytes): name and short are the empty string, the template (8) is `RESERVE_TEMPLATE`, the slot (0xc) is `SLOT_BASE`;
/// - Digitone II page row (`0x60_0000`, 0x80 bytes): name and title are the empty representation, the level code after the eight knob codes (0x28) is 10, the machine
///   (0x70) is the free machine `PG_FREE` (0xff).
fn kept(_core: &FakeCore, addr: u32, value: u32) -> bool {
    const EMPTY: u32 = 0x50_0000;
    let rep = tagged(addr_of!(reg().empty_rep) as usize + 12);
    let in_rows = |base: u32, size: u32, n: usize| {
        (addr >= base && ((addr - base) / size) < n as u32).then(|| (addr - base) % size)
    };
    if let Some(off) = in_rows(0x30_0000, 0x100, limits::MACHINE_ROWS) {
        let s = &reg().s;
        let inside =
            |base: usize, size: usize| value >= tagged(base) && value < tagged(base) + size as u32;
        return match off {
            0 => value == EMPTY,
            0x4c => inside(
                addr_of!(s.mach_short) as usize,
                core::mem::size_of_val(&s.mach_short),
            ),
            0x48 => value == 1,
            4 => {
                value == rep
                    || inside(
                        addr_of!(s.rep_long) as usize,
                        core::mem::size_of_val(&s.rep_long),
                    )
            }
            8 => {
                value == rep
                    || inside(
                        addr_of!(s.rep_short) as usize,
                        core::mem::size_of_val(&s.rep_short),
                    )
            }
            0x30 => value == 10,
            0x44 => true,
            _ => false,
        };
    }
    if let Some(off) = in_rows(0x20_0000, 0x20, limits::PARAM_ROWS) {
        return match off {
            0 | 4 => value == EMPTY,
            8 => value == 7,
            0xc => value == 25,
            _ => false,
        };
    }
    if let Some(off) = in_rows(0x60_0000, 0x80, limits::PAGE_ROWS) {
        return match off {
            0 | 4 => value == rep,
            0x28 => value == 10,
            0x70 => value == 0xff,
            _ => false,
        };
    }
    let names = 0x40_0000 + 12 * DYN_TYPE_FIRST;
    if let Some(off) = in_rows(names, 12, limits::MACHINE_ROWS) {
        let s = &reg().s;
        let inside =
            |base: usize, size: usize| value >= tagged(base) && value < tagged(base) + size as u32;
        return match off {
            0 => {
                value == EMPTY
                    || inside(
                        addr_of!(s.mach_name) as usize,
                        core::mem::size_of_val(&s.mach_name),
                    )
            }
            4 => {
                value == EMPTY
                    || inside(
                        addr_of!(s.mach_short) as usize,
                        core::mem::size_of_val(&s.mach_short),
                    )
            }
            _ => false,
        };
    }
    false
}

fn keys_of(mk: &[u32]) -> KeysCopy {
    let mut k = KeysCopy::empty();
    k.machine_rows = 1;
    k.param_rows = 2;
    k.page_rows = 1;
    k.nm = mk.len();
    k.mk[..mk.len()].copy_from_slice(mk);
    k
}
fn keys_a() -> KeysCopy {
    keys_of(&[KEY_A])
}
fn keys_b() -> KeysCopy {
    keys_of(&[KEY_B])
}
/// The number of the first run-time machine, pinned (a key with hash 0): no sticky map entry holds it.
fn keys_pinned() -> KeysCopy {
    keys_of(&[DYN_TYPE_FIRST])
}
/// Two more pinned numbers (hash 0), in the run-time range, in no map entry.
fn keys_pinned_other() -> KeysCopy {
    keys_of(&[DYN_TYPE_FIRST + 5])
}
fn keys_pinned_third() -> KeysCopy {
    keys_of(&[DYN_TYPE_FIRST + 6])
}
fn keys_late() -> KeysCopy {
    keys_of(&[KEY_L1])
}
/// A keys table without the machine's row: the commit refuses the builder calls (`E_DECL_ID`).
fn keys_none() -> KeysCopy {
    keys_of(&[])
}

const P_TABLE: u32 = 0x10_0000;
const P_FIRST: u32 = 300;

/// The words of `dynreg_info` the machines service reads, for the profile of `core.device`, and the voice rows of `dyndsp_info`.
fn image(core: &mut FakeCore) {
    let dn2 = core.device == DN2.id;
    let mut t = vec![0u32; 96];
    t[w::VERSION] = 3;
    t[w::DEVICE] = core.device;
    t[w::M_ROWS] = 0x30_0000;
    t[w::M_ROW_SIZE] = 0x100;
    t[w::M_FIRST] = DYN_TYPE_FIRST;
    t[w::M_DYN0] = DYN_TYPE_FIRST;
    t[w::M_DYN_N] = limits::MACHINE_ROWS as u32;
    t[w::NAMES] = 0x40_0000;
    t[w::PERMIT] = 0x41_0000;
    t[w::PERTYPE] = 0x42_0000;
    t[w::O_LONG] = 4;
    t[w::O_SHORT] = 8;
    t[w::O_F0] = 0x10;
    t[w::O_CLONE] = 0x40;
    t[w::O_POS] = 0x44;
    t[w::O_OWN] = 0x48;
    t[w::O_SNAME] = 0x4c;
    t[w::P_TABLE] = P_TABLE;
    t[w::P_ROWS] = 0x20_0000;
    t[w::P_ROW_SIZE] = 0x20;
    t[w::P_FIRST] = P_FIRST;
    t[w::P_DYN0] = P_FIRST;
    t[w::P_DYN_N] = limits::PARAM_ROWS as u32;
    t[w::PF_SIZE] = 0x40;
    t[w::P_TEMPLATE_OFF] = 8;
    t[w::EMPTY] = 0x50_0000;
    t[w::SPEC] = 0x100;
    t[w::SPEC + 1] = 0x101;
    t[w::SPEC + 2] = 0x102;
    t[w::KNOBS] = 8;
    t[w::RESERVE_TEMPLATE] = 7;
    if dn2 {
        // the Digitone II keeps a machine's knobs in page rows
        t[w::SLOT_BASE] = 25;
        t[w::PG_ROWS] = 0x60_0000;
        t[w::PG_ROW_SIZE] = 0x80;
        t[w::PG_FIRST] = 0;
        t[w::PG_DYN0] = 0;
        t[w::PG_DYN_N] = limits::PAGE_ROWS as u32;
        t[w::PGO_NAME] = 0;
        t[w::PGO_TITLE] = 4;
        t[w::PGO_C] = 8;
        t[w::PGO_S] = 0x2c;
        t[w::PGO_MACH] = 0x70;
        t[w::PG_FREE] = 0xff;
    }
    // the builder profile (`Profile` of `tests/build.rs`)
    t[w::P_CLONE] = 3;
    t[w::P_LEVEL] = 10;
    t[w::P_OWN] = dn2 as u32;
    t[w::P_TUNE] = if dn2 { 0 } else { 0xca };
    t[w::P_LEVEL_CODE] = if dn2 { 0 } else { 0xd1 };
    t[w::P_LEVEL_KNOB] = if dn2 { 0 } else { 7 };
    t[w::P_NSLOTS] = if dn2 { 8 } else { 6 };
    let slots: [u32; 8] = if dn2 {
        [1, 2, 4, 5, 6, 0, 3, 7]
    } else {
        [1, 2, 4, 5, 6, 7, 0, 0]
    };
    t[w::P_SLOTS..w::P_SLOTS + 8].copy_from_slice(&slots);
    let tpl: [u32; 6] = if dn2 {
        [0xd5, 0xd7, 0x47, 0xdd, 0xee, 0xd5]
    } else {
        [0xcc, 0xd1, 0x47, 0x32, 0xca, 0xcb]
    };
    t[w::P_TPL..w::P_TPL + 6].copy_from_slice(&tpl);
    core.set_dynreg(t);
    // the per-type tables of the firmware hold the free row's words for every type, and the clone's (type 3) are other: a machine's rows take the clone's words,
    // and a release gives the free row's back
    {
        let mut m = core.mem.borrow_mut();
        for t in 0..256u32 {
            let clone = t == 3;
            m.insert(0x40_0000 + 12 * t + 8, if clone { 0x99 } else { 0x77 }); // the names table: word 2 of each entry
            m.insert(0x41_0000 + 4 * t, if clone { 0x55 } else { 0x11 }); // the permit table
        }
        for word in 0..64u32 {
            // the per-type bytes: 1 for every type, 5 for type 3
            let byte = |t: u32| if t == 3 { 5u32 } else { 1 };
            m.insert(
                0x42_0000 + 4 * word,
                (0..4).fold(0, |v, i| v << 8 | byte(4 * word + i)),
            );
        }
    }
    // `dyndsp_info`: the voice rows start at row 2 of the DSP's table and four are free
    core.set_dyndsp(0x10_0000, 2, 4);
    // `machines_info`: the adapter's native page register is bound (a call of it is a firmware call the fake answers)
    let mut mi = vec![0u32; dkrt_svc_machines::info::WORDS];
    mi[dkrt_svc_machines::info::w::MAGIC] = dkrt_svc_machines::info::MAGIC;
    mi[dkrt_svc_machines::info::w::VERSION] = dkrt_svc_machines::info::VERSION;
    // the owner binds the table at its start: the device it runs as, and the two words the hooks cannot do without
    mi[dkrt_svc_machines::info::w::DEVICE] = core.device;
    mi[dkrt_svc_machines::info::w::ENSURE] = 0x4000_1000;
    mi[dkrt_svc_machines::info::w::ROWS] = 0x4200_0000;
    mi[dkrt_svc_machines::info::w::N_REGISTER] = 0x6000_0010;
    mi[dkrt_svc_machines::info::w::N_REGISTER_V2] = 0x6000_0020;
    core.info_tables.push((name_hash("machines_info"), mi));
}

/// The service's own view: who owns the machine, parameter and page rows, and the type numbers each owner has.
fn state(core: &FakeCore) -> Vec<u32> {
    let s = reg();
    let mut v: Vec<u32> = (0..limits::MACHINE_ROWS)
        .map(|k| mach_owner(core, k) as u32)
        .collect();
    v.extend(s.param_owner.iter().map(|o| *o as u32));
    v.extend(s.page_owner.iter().map(|o| *o as u32));
    for owner in 1..=3u8 {
        let mut ids = [0u8; 8];
        let n = types_of(core, owner, &mut ids) as u32;
        v.push(n);
        v.extend(ids.iter().take(n as usize).map(|i| *i as u32));
    }
    v
}

/// The row bytes an unload leaves (docs/plans/extension-model.md E5d): the voice row of an absent module whose number the sticky map holds stays as a MISSING placeholder.
/// Only the bytes of such a row are accepted: the owner byte `PLACEHOLDER_OWNER`, the row's type (a held number) and the placeholder pc; `reset_row` clears every other word.
/// The offsets are those of `VoiceRows`.
fn kept_rows(_core: &FakeCore, off: usize, value: u8) -> bool {
    use core::mem::offset_of;
    use dkrt_abi::dsp::{MAX_ROWS, PLACEHOLDER_OWNER, PLACEHOLDER_PC};
    let d = vrows();
    let row_of = |base: usize, size: usize| {
        (off >= base && off < base + size * MAX_ROWS)
            .then(|| ((off - base) / size, (off - base) % size))
    };
    if let Some((r, _)) = row_of(offset_of!(VoiceRows, row_owner), 1) {
        return d.row_owner[r] == PLACEHOLDER_OWNER && value == PLACEHOLDER_OWNER;
    }
    if let Some((r, i)) = row_of(offset_of!(VoiceRows, row_type), 4) {
        return d.row_owner[r] == PLACEHOLDER_OWNER
            && dkrt_svc_machines::assign::holds(d.row_type[r])
            && value == d.row_type[r].to_ne_bytes()[i];
    }
    if let Some((r, i)) = row_of(offset_of!(VoiceRows, row_pc), 4) {
        return d.row_owner[r] == PLACEHOLDER_OWNER && value == PLACEHOLDER_PC.to_ne_bytes()[i];
    }
    false
}

/// The voice rows of a snapshot's bytes.
fn rows_of(bytes: &[u8]) -> VoiceRows {
    assert_eq!(bytes.len(), core::mem::size_of::<VoiceRows>());
    unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const VoiceRows) }
}

fn bytes_of(v: &VoiceRows) -> Vec<u8> {
    unsafe {
        core::slice::from_raw_parts(
            v as *const VoiceRows as *const u8,
            core::mem::size_of::<VoiceRows>(),
        )
    }
    .to_vec()
}

/// The rows with each MISSING placeholder voice row as a free row (owner 0, type `FREE_TYPE`, every other field 0): what a comparison of services that park rows of
/// absent modules leaves of the row bytes that are not the module's own.
fn without_placeholders(bytes: &[u8]) -> Vec<u8> {
    let mut d = rows_of(bytes);
    for k in 0..dkrt_abi::dsp::MAX_ROWS {
        if d.row_owner[k] == dkrt_abi::dsp::PLACEHOLDER_OWNER {
            d.row_owner[k] = 0;
            d.row_type[k] = FREE_TYPE;
            d.row_pc[k] = 0;
            d.row_rng[k] = [0; 8];
            d.row_pub[k] = 0;
            d.row_note_on[k] = 0;
            d.row_state_base[k] = 0;
            d.row_stride[k] = 0;
            d.row_clear[k] = 0;
            d.row_latched[k] = 0;
        }
    }
    bytes_of(&d)
}

/// The voice rows in the order of their types (free rows last), with the row masks of the records following: a module loaded beside a parked row has its row at another
/// index than the same module loaded alone, and its words are compared by type.
fn by_type(bytes: &[u8]) -> Vec<u8> {
    let d = rows_of(bytes);
    let mut order: Vec<usize> = (0..dkrt_abi::dsp::MAX_ROWS).collect();
    order.sort_by_key(|k| (d.row_type[*k], *k));
    let mut n = rows_of(bytes);
    for (i, k) in order.iter().enumerate() {
        n.row_type[i] = d.row_type[*k];
        n.row_pc[i] = d.row_pc[*k];
        n.row_rng[i] = d.row_rng[*k];
        n.row_owner[i] = d.row_owner[*k];
        n.row_pub[i] = d.row_pub[*k];
        n.row_note_on[i] = d.row_note_on[*k];
        n.row_state_base[i] = d.row_state_base[*k];
        n.row_stride[i] = d.row_stride[*k];
        n.row_clear[i] = d.row_clear[*k];
        n.row_latched[i] = d.row_latched[*k];
    }
    for r in 0..n.mod_rows.len() {
        let mut m = 0;
        for (i, k) in order.iter().enumerate() {
            if d.mod_rows[r] & (1 << k) != 0 {
                m |= 1 << i;
            }
        }
        n.mod_rows[r] = m;
    }
    bytes_of(&n)
}

/// Voice rows that are MISSING placeholders in the bytes.
fn placeholder_rows(bytes: &[u8]) -> usize {
    rows_of(bytes)
        .row_owner
        .iter()
        .filter(|o| **o == dkrt_abi::dsp::PLACEHOLDER_OWNER)
        .count()
}

/// (placeholder voice rows, numbers the map holds that no module has): the rows of absent modules are parked for held numbers only.
fn absent_rows(core: &FakeCore) -> (usize, usize) {
    let d = vrows();
    let rows = rows_owned_by(core, dkrt_abi::dsp::PLACEHOLDER_OWNER).len();
    let held = (0..dkrt_sdk::sel::MAP_N)
        .map(dkrt_svc_machines::assign::number_at)
        .filter(|t| {
            *t != 0
                && !(0..dkrt_abi::dsp::MAX_ROWS).any(|k| {
                    d.row_owner[k] != 0
                        && d.row_owner[k] != dkrt_abi::dsp::PLACEHOLDER_OWNER
                        && d.row_type[k] == *t as u32
                })
        })
        .count();
    (rows, held)
}

fn module(sel: u32, keys: fn() -> KeysCopy, init: fn(&mut Init), salt: u32) -> Module {
    Module {
        sel,
        keys,
        groups: &[],
        init,
        dsp: Dsp {
            voices: 1,
            salt,
            ..Dsp::default()
        },
    }
}

fn fixture(device: u32) -> Fixture {
    Fixture {
        name: "sine",
        device,
        kinds: &KINDS_MACHINES,
        image,
        state,
        a: module(0, keys_a, sine, 1),
        b: module(1, keys_b, sine_two, 3),
        a2: module(0, keys_a, sine_alt, 2),
        enable: &ENABLE,
        parks_rows: true,
        kept,
        kept_dsp: |_, _, _| false, // the loader's state (`DspState`) keeps nothing of a voice row: the rows are the service's own (`rows`)
        rows: Rows {
            bytes: || rows::bytes().to_vec(),
            reset: rows::reset,
            kept: kept_rows,
            without_placeholders,
            by_type,
            placeholder_rows,
        },
        absent_rows: Some(absent_rows),
        // the machines service queues no live value for its rows
        live: None,
        stateless: false,
        salt_same: &[
            ("settings", "a load writes no setting"),
            (
                "state",
                "a2 has the keys of A: the same rows, owners and type ids",
            ),
        ],
        bad: module(2, keys_none, sine, 1),
        bad_late: Some(Late {
            bad: module(3, keys_late, native_page_fails, 1),
            same_but_ok: module(3, keys_late, native_page_ok, 1),
            beside_b: true,
        }),
        rules: true,
        parked_knobs: None,
        survives: None,
    }
}

#[test]
fn the_machines_service_passes_the_contract_on_the_dt2() {
    provider_contract::<MachinesRecipe>(&fixture(DT2.id));
}

#[test]
fn the_machines_service_passes_the_contract_on_the_dn2() {
    provider_contract::<MachinesRecipe>(&fixture(DN2.id));
}

// ---- the MISSING placeholder (docs/plans/extension-model.md E5 item 7) -------------------------------------------------------------------

/// The last word written to (window, word) on the link, `None` when none was.
fn link_word(core: &FakeCore, win: u32, word: u32) -> Option<u32> {
    core.link
        .borrow()
        .t
        .wire
        .iter()
        .rev()
        .find(|w| w.win == win && w.dst == word)
        .map(|w| u32::from_le_bytes([w.bytes[0], w.bytes[1], w.bytes[2], w.bytes[3]]))
}

fn voice_row_word(core: &FakeCore, row: usize, word: u32) -> Option<u32> {
    let first = unsafe { dkrt_abi::mem::iw(dyndsp(core), I_VFIRST) };
    link_word(core, WIN_VOICES, ROW_WORDS * (first + row as u32) + word)
}

fn rows_owned_by(_core: &FakeCore, owner: u8) -> Vec<usize> {
    (0..dkrt_abi::dsp::MAX_ROWS)
        .filter(|k| vrows().row_owner[*k] == owner)
        .collect()
}

/// The sticky map forgets every number, then holds those of `mods` only (a boot that does not know the module, or a number the map dropped): the settings key is zeroed and
/// `settle` runs over `mods`.
fn forget_numbers_but(l: &Lifecycle<Provided<MachinesRecipe>>, mods: &[&Module]) {
    l.core
        .settings
        .borrow_mut()
        .retain(|(k, _)| k.as_slice() != dkrt_svc_machines::assign::SETTING);
    l.settle_over(mods);
}

#[test]
fn a_voice_row_of_an_absent_module_is_a_disabled_placeholder_that_keeps_its_type_while_the_map_holds_its_number(
) {
    use dkrt_abi::dsp::{PLACEHOLDER_OWNER, PLACEHOLDER_PC};
    use dkrt_test_support::contract::locked;
    let _g = locked();
    for dev in [DT2.id, DN2.id] {
        let f = fixture(dev);
        let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
        assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
        let row = rows_owned_by(&l.core, 1)[0];
        let ty = vrows().row_type[row];
        assert_eq!(
            voice_row_word(&l.core, row, 0),
            Some(ty),
            "the row is published with its type"
        );
        l.core.live_op.set(1);
        l.unload(0);
        let d = vrows();
        assert_eq!(
            (d.row_owner[row], d.row_type[row], d.mod_rows[0]),
            (PLACEHOLDER_OWNER, ty, 0),
            "dev {dev}: parked, not freed"
        );
        // the DSP: the type word stays, the disabled word (10) names the type, the pc is the placeholder's
        assert_eq!(voice_row_word(&l.core, row, 0), Some(ty));
        assert_eq!(
            voice_row_word(&l.core, row, 10),
            Some(ty),
            "the dispatch renders a silent block for it"
        );
        assert_eq!(voice_row_word(&l.core, row, 1), Some(PLACEHOLDER_PC));
        // a placeholder row is no row in use in the CAPACITY figure (it gives way to a real load, so the planner counts it free): the device test reads that figure against the row masks
        let used = |l: &Lifecycle<Provided<MachinesRecipe>>| {
            unsafe { l.driver.cap(&l.core, dkrt_sdk::proto::CAPK_VOICE_ROWS) }
                .unwrap()
                .1
        };
        assert_eq!(
            (used(&l), vrows().mod_rows[0]),
            (0, 0),
            "a parked row is not in use and in no module's row mask"
        );
        // the reload ended without the module: its number is in the sticky map, so the row stays parked (the module is absent: MISSING and silent, E5d)
        unsafe { l.driver.live_done(&l.core) };
        let d = vrows();
        assert_eq!((d.row_owner[row], d.row_type[row]), (PLACEHOLDER_OWNER, ty));
        assert_eq!(used(&l), 0);
        assert_eq!(rows_owned_by(&l.core, PLACEHOLDER_OWNER), vec![row]);
        assert_eq!(voice_row_word(&l.core, row, 10), Some(ty));
        // the number leaves the map (settings forgotten, a settle that does not name it): the next end of a reload frees the row, in the DSP too
        forget_numbers_but(&l, &[]);
        unsafe { l.driver.live_done(&l.core) };
        let d = vrows();
        assert_eq!((d.row_owner[row], d.row_type[row]), (0, FREE_TYPE));
        assert_eq!(used(&l), 0);
        assert_eq!(voice_row_word(&l.core, row, 0), Some(FREE_TYPE));
        assert_eq!(voice_row_word(&l.core, row, 10), Some(0));
    }
}

#[test]
fn a_plain_unload_parks_the_voice_row_and_shows_missing_while_the_map_holds_the_number() {
    use dkrt_abi::dsp::{PLACEHOLDER_OWNER, PLACEHOLDER_PC};
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    let ty = vrows().row_type[row];
    l.unload(0); // no reload runs: the unload of a module taken down from the menu, or the boot's failure path (`undo_early`)
    let d = vrows();
    assert_eq!(
        (d.row_owner[row], d.row_type[row]),
        (PLACEHOLDER_OWNER, ty),
        "a parked row, no free row"
    );
    assert_eq!(voice_row_word(&l.core, row, 0), Some(ty));
    assert_eq!(voice_row_word(&l.core, row, 10), Some(ty), "disabled");
    assert_eq!(voice_row_word(&l.core, row, 1), Some(PLACEHOLDER_PC));
    // the machine row reads MISSING and stays out of the list (empty list name), the DSP gets the row's own number
    assert_eq!(missing_row(&l.core), (true, true, true));
    // the module loads again: it takes the row and the text is its own
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    assert_eq!(rows_owned_by(&l.core, 1), vec![row]);
    assert_eq!(missing_row(&l.core), (false, false, false));
    l.unload(0);
}

#[test]
fn a_plain_unload_of_a_number_the_map_does_not_hold_frees_the_voice_row_and_leaves_a_gap_row() {
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    forget_numbers_but(&l, &[]);
    // a pinned number is in no map entry
    let pinned = module(0, keys_pinned, sine, 1);
    assert_eq!(l.load(0, &pinned), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    l.unload(0);
    let d = vrows();
    assert_eq!(
        (d.row_owner[row], d.row_type[row]),
        (0, FREE_TYPE),
        "a free row, no placeholder"
    );
    assert_eq!(voice_row_word(&l.core, row, 0), Some(FREE_TYPE));
    assert!(rows_owned_by(&l.core, dkrt_abi::dsp::PLACEHOLDER_OWNER).is_empty());
    // the machine row is the gap row: hidden, no MISSING text, own_dsp 0
    assert_eq!(missing_row(&l.core), (true, false, false));
}

#[test]
fn a_number_in_the_map_that_no_module_has_reads_missing_and_is_parked_once_the_loader_is_idle() {
    use dkrt_abi::dsp::{PH_IDLE, PLACEHOLDER_OWNER, PLACEHOLDER_PC};
    use dkrt_abi::service::tick_phase;
    use dkrt_test_support::contract::locked;
    let _g = locked();
    for dev in [DT2.id, DN2.id] {
        let f = fixture(dev);
        // the boot's settle numbered A and B (the map holds both), and neither loaded: a module not on the card, or one that failed before its rows were written
        let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
        let tick = || unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
        // the loader is busy with a module: the machine rows are written, the voice rows wait
        l.core.dsp().phase = PH_IDLE + 1;
        tick();
        assert!(
            !missing_row(&l.core).2,
            "dev {dev}: no MISSING row before the voice row is parked"
        );
        assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
        l.core.dsp().phase = PH_IDLE;
        tick();
        let rows = rows_owned_by(&l.core, PLACEHOLDER_OWNER);
        assert_eq!(
            rows.len() as u32,
            dkrt_svc_machines::assign::map_used(),
            "dev {dev}: one parked row for each absent number"
        );
        let d = vrows();
        for r in &rows {
            assert_eq!(d.row_pc[*r], PLACEHOLDER_PC);
            assert_eq!(
                voice_row_word(&l.core, *r, 10),
                Some(d.row_type[*r]),
                "disabled"
            );
            assert_eq!(voice_row_word(&l.core, *r, 0), Some(d.row_type[*r]));
        }
        // nothing more is written by another pass
        let writes = l.core.link.borrow().t.wire.len();
        tick();
        assert_eq!(l.core.link.borrow().t.wire.len(), writes);
        // A loads: it takes its placeholder and its machine row is its own again
        assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
        assert_eq!(
            rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(),
            rows.len() - 1
        );
        let own = missing_row(&l.core);
        assert_eq!(
            (own.0, own.1),
            (false, false),
            "the row is A's own: listed, its own text"
        );
        tick();
        assert_eq!(
            rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(),
            rows.len() - 1
        );
        assert_eq!(rows_owned_by(&l.core, 1).len(), 1);
        l.unload(0);
    }
}

#[test]
fn a_removal_writes_missing_only_when_a_voice_row_is_parked_and_a_busy_loader_is_bounded() {
    use dkrt_abi::dsp::{PH_IDLE, PLACEHOLDER_OWNER};
    use dkrt_abi::service::tick_phase;
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    let tick = || unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
    let whys = || {
        l.core
            .calls
            .borrow()
            .iter()
            .filter_map(|c| match c {
                dkrt_test_support::Call::Log { kind, args, .. }
                    if *kind == dkrt_sdk::logk::MACHINE_TYPE_EVICTED =>
                {
                    Some(args[2])
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // the unload of a module whose voice row was never published: no voice row stands for the number, so the row is the gap row, not MISSING
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    vrows().row_pub[row] = 0;
    l.unload(0);
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
    assert!(
        !missing_row(&l.core).2,
        "no MISSING row without a voice row"
    );
    // the loader stays busy: nothing is parked, the row stays a gap row, and after the bound one event says why (once)
    l.core.dsp().phase = PH_IDLE + 1;
    for _ in 0..1799 {
        tick();
    }
    assert!(whys().is_empty());
    tick();
    tick();
    assert_eq!(
        whys().iter().filter(|w| **w == 4).count() as u32,
        dkrt_svc_machines::assign::map_used(),
        "one event for each held number, logged once"
    );
    assert!(!missing_row(&l.core).2);
    // the loader is idle again: the number is parked and reads MISSING
    l.core.dsp().phase = PH_IDLE;
    tick();
    assert!(missing_row(&l.core).2);
    assert_eq!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER).len() as u32,
        dkrt_svc_machines::assign::map_used()
    );
}

/// The parking passes belong to a plan of the numbers: a new plan (`settle`) starts them again, and the end of a reload (`reload_done`) forgets the tries.
#[test]
fn settle_forgets_the_parking_state_of_the_earlier_plan() {
    use dkrt_abi::dsp::PH_IDLE;
    use dkrt_abi::service::{tick_phase, SettleCtx};
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    vrows().row_pub[row] = 0;
    l.unload(0);
    // a busy loader: every pass counts
    l.core.dsp().phase = PH_IDLE + 1;
    for _ in 0..3 {
        unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
    }
    let seen = || {
        l.driver
            .with_instance(|m| (m.parking().busy_passes(), m.parking().tries()))
    };
    assert_eq!(seen().0, 3, "the passes are counted");
    unsafe { l.driver.settle(&l.core, &SettleCtx::new()) };
    assert_eq!(seen(), (0, 0), "a new plan starts them again");
}

#[test]
fn a_real_load_takes_the_row_of_a_placeholder_and_the_number_is_parked_again_when_a_row_frees() {
    use dkrt_abi::dsp::PLACEHOLDER_OWNER;
    use dkrt_abi::service::tick_phase;
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    let tick = || unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
    // every row is a placeholder: the boot's numbers (A, B and the two refused modules) are absent
    tick();
    let held = dkrt_svc_machines::assign::map_used() as usize;
    assert_eq!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(), held);
    let free = unsafe { dkrt_abi::mem::iw(dyndsp(&l.core), I_VFREE) } as usize;
    // modules of numbers that have no row (pinned, in no map entry) load until the rows are full: a placeholder gives way to a real load, never the other way
    for (rec, keys) in [
        (2usize, keys_pinned_other as fn() -> KeysCopy),
        (3, keys_pinned_third),
    ] {
        let m = module(rec as u32, keys, sine, 3);
        assert_eq!(
            l.load(rec, &m),
            dkrt_sdk::OK,
            "a placeholder gives way to a real load"
        );
    }
    assert_eq!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(),
        held.min(free - 2)
    );
    // the number that lost its row: the gap row and an error event
    let gave: Vec<(u32, u32)> = l
        .core
        .calls
        .borrow()
        .iter()
        .filter_map(|c| match c {
            dkrt_test_support::Call::Log { kind, args, .. }
                if *kind == dkrt_sdk::logk::MACHINE_TYPE_EVICTED =>
            {
                Some((args[1], args[2]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        gave.len(),
        held + 2 - free,
        "one event for each row a load took"
    );
    assert_eq!(gave[0].1, 1, "reason: a real load took the row");
    let lost = gave[0].0;
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER)
        .iter()
        .all(|r| vrows().row_type[*r] != lost));
    tick();
    assert_eq!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(),
        held.min(free - 2),
        "no row is free: the number stays without one"
    );
    // a module goes: its row is free, and the number is parked again at the next pass
    l.unload(3);
    tick();
    assert_eq!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(),
        held.min(free - 1)
    );
    l.unload(2);
}

#[test]
fn a_boot_that_fits_exactly_loads_every_selected_module_beside_a_parked_number() {
    use dkrt_abi::dsp::PLACEHOLDER_OWNER;
    use dkrt_abi::service::tick_phase;
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    let free = unsafe { dkrt_abi::mem::iw(dyndsp(&l.core), I_VFREE) } as usize;
    // the first passes: the boot's loads are not over (a load is in flight), nothing is parked, so no placeholder stands in the way of a later module
    l.core.boot_index.set(0);
    unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
    l.core.boot_index.set(u32::MAX);
    // `free` modules fill every row: the boot fits exactly, and one more number is held for an absent module
    let mods = [(0usize, &f.a), (1, &f.b)];
    for (rec, m) in mods {
        assert_eq!(l.load(rec, m), dkrt_sdk::OK);
    }
    let p1 = module(2, keys_pinned_other, sine, 3);
    let p2 = module(3, keys_pinned_third, sine, 4);
    assert_eq!(l.load(2, &p1), dkrt_sdk::OK);
    assert_eq!(l.load(3, &p2), dkrt_sdk::OK);
    assert_eq!(free, 4);
    unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
    assert!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty(),
        "no free row: no placeholder, every module loaded"
    );
    for rec in (0..4).rev() {
        l.unload(rec);
    }
}

#[test]
fn the_parked_rows_of_absent_numbers_are_bounded_by_the_free_rows_and_no_pass_runs_during_a_live_operation(
) {
    use dkrt_abi::dsp::PLACEHOLDER_OWNER;
    use dkrt_abi::service::tick_phase;
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    let tick = || unsafe { l.driver.tick(&l.core, tick_phase::LAST) };
    // a live operation: nothing is written
    l.core.live_op.set(1);
    tick();
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
    assert!(!missing_row(&l.core).2, "no MISSING row was written");
    l.core.live_op.set(0);
    // only one voice row is free: the first absent number is parked, the other waits without an error (it plays what a gap row plays until a row is free)
    let inf = unsafe { dyndsp(&l.core) };
    let free = unsafe { dkrt_abi::mem::iw(inf, I_VFREE) } as usize;
    for k in 1..free {
        vrows().row_owner[k] = 9;
        vrows().row_type[k] = 0x77 + k as u32;
    }
    tick();
    assert_eq!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).len(), 1);
    // the numbers without a row keep the gap row (not MISSING: that would play the stock sound), and one error event for each says why
    let events = || {
        l.core
            .calls
            .borrow()
            .iter()
            .filter_map(|c| match c {
                dkrt_test_support::Call::Log { kind, args, .. }
                    if *kind == dkrt_sdk::logk::MACHINE_TYPE_EVICTED =>
                {
                    Some((args[1], args[2]))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let held = dkrt_svc_machines::assign::map_used() as usize;
    assert_eq!(
        events().len(),
        held - 1,
        "one event for each number without a row"
    );
    assert!(
        events().iter().all(|(_, why)| *why == 2),
        "the reason is: no free row"
    );
    tick();
    assert_eq!(
        events().len(),
        held - 1,
        "an event is logged once, not at every pass"
    );
    for k in 1..free {
        vrows().row_owner[k] = 0;
        vrows().row_type[k] = FREE_TYPE;
    }
    tick();
    assert_eq!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER).len() as u32,
        dkrt_svc_machines::assign::map_used()
    );
}

/// The machine row of the first run-time type: (its list name is empty, its long name reads MISSING, own_dsp is 1).
fn missing_row(core: &FakeCore) -> (bool, bool, bool) {
    let s = &reg().s;
    let inf = unsafe { dkrt_svc_rows::dynreg(core) };
    let (empty, own, name) = unsafe {
        let row = dkrt_abi::mem::iw(inf, w::M_ROWS)
            + (DYN_TYPE_FIRST - dkrt_abi::mem::iw(inf, w::M_FIRST))
                * dkrt_abi::mem::iw(inf, w::M_ROW_SIZE);
        (
            dkrt_abi::mem::iw(inf, w::EMPTY),
            core.rd(row + dkrt_abi::mem::iw(inf, w::O_OWN)),
            core.rd(row + dkrt_abi::mem::iw(inf, w::O_NAME)),
        )
    };
    (name == empty, s.mach_name[0][0] == b'M', own == 1)
}

#[test]
fn the_module_of_the_same_number_takes_its_placeholder_row_and_another_module_leaves_it() {
    use dkrt_abi::dsp::PLACEHOLDER_OWNER;
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    l.core.live_op.set(1);
    l.unload(0);
    // module B (another key) loads: A's placeholder stays, B has its own row
    assert_eq!(l.load(1, &f.b), dkrt_sdk::OK);
    assert_eq!(
        vrows().row_owner[row],
        PLACEHOLDER_OWNER,
        "B did not take A's placeholder"
    );
    assert!(!rows_owned_by(&l.core, 2).contains(&row));
    // A loads again (its number is not refused for the placeholder): it takes its row, and the disabled word is cleared
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let d = vrows();
    assert_eq!(d.row_owner[row], 1, "A's row is the one it had");
    assert_eq!(
        voice_row_word(&l.core, row, 10),
        Some(0),
        "the disabled word is cleared when the row is published"
    );
    assert_eq!(voice_row_word(&l.core, row, 0), Some(d.row_type[row]));
    // that the row's enable words and what the service holds are those of the first load is the contract suite's case 12b; nothing is left to free at the end
    unsafe { l.driver.live_done(&l.core) };
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
    l.core.live_op.set(0);
    l.unload(0);
    l.unload(1);
}

#[test]
fn many_types_loaded_and_reloaded_do_not_use_up_the_voice_rows() {
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    let used = |l: &Lifecycle<Provided<MachinesRecipe>>| {
        unsafe { l.driver.cap(&l.core, dkrt_sdk::proto::CAPK_VOICE_ROWS) }
            .unwrap()
            .1
    };
    for round in 0..24 {
        for (rec, m) in [(0usize, &f.a), (1, &f.b)] {
            assert_eq!(
                l.load(rec, m),
                dkrt_sdk::OK,
                "round {round}: a load is refused for rows"
            );
        }
        // a reload of each, then the plain unload of the boot's failure path
        for rec in 0..2 {
            l.core.live_op.set(1);
            l.unload(rec);
            assert_eq!(
                l.load(rec, if rec == 0 { &f.a } else { &f.b }),
                dkrt_sdk::OK
            );
            unsafe { l.driver.live_done(&l.core) };
            l.core.live_op.set(0);
        }
        assert_eq!(
            used(&l),
            2,
            "round {round}: two voice rows, no placeholder left"
        );
        l.unload(0);
        l.unload(1);
        assert_eq!(
            used(&l),
            0,
            "round {round}: the two numbers are absent: their rows stay parked (placeholders are no rows in use)"
        );
    }
}

#[test]
fn a_placeholder_is_a_free_row_for_a_real_load() {
    use dkrt_abi::dsp::{DspSec, VOICE_TYPE, VOICE_V3};
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(1, &f.b), dkrt_sdk::OK); // one row
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK); // one row, which the reload parks
    l.core.live_op.set(1);
    l.unload(0);
    // 4 rows: B's, A's placeholder, 2 free. A placeholder is free for any real load, so a file with 3 voices fits (whatever their types) and one with 4 does not
    let check = |keys: &[u32]| {
        let n = keys.len() as u32;
        let mut bytes = vec![0u8; VOICE_V3 as usize * keys.len()];
        for (i, k) in keys.iter().enumerate() {
            let at = VOICE_V3 as usize * i + VOICE_TYPE as usize;
            bytes[at..at + 4].copy_from_slice(&k.to_be_bytes());
        }
        let sec = DspSec {
            s: bytes.as_ptr(),
            version: 5,
            size: bytes.len() as u32,
            image_size: 0,
            image_off: 0,
            reloc_off: 0,
            nreloc: 0,
            import_off: 0,
            nimport: 0,
            voice_off: 0,
            nvoice: n,
            fx_off: 0,
            nfx: 0,
            vstride: VOICE_V3,
            bank_off: 0,
        };
        l.core.boot_index.set(0);
        let r = unsafe { l.driver.dsp_check_rows(&l.core, &sec) };
        l.core.boot_index.set(u32::MAX);
        r
    };
    assert_eq!(check(&[KEY_A, 0x00dd_0000, 0x00ee_0000]), dkrt_sdk::OK);
    assert_eq!(
        check(&[0x00cc_0000, 0x00dd_0000, 0x00ee_0000]),
        dkrt_sdk::OK,
        "the placeholder of A is free for a voice of another type"
    );
    assert_eq!(
        check(&[0x00cc_0000, 0x00dd_0000, 0x00ee_0000, 0x00ff_0000]),
        dkrt_sdk::E_DSP_ROWS
    );
    unsafe { l.driver.live_done(&l.core) };
    l.core.live_op.set(0);
    l.unload(1);
}

#[test]
fn a_failed_publication_into_a_reused_placeholder_row_disables_it_again() {
    use dkrt_abi::dsp::{PLACEHOLDER_OWNER, ROW_STEPS, ROW_WORDS};
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    let ty = vrows().row_type[row];
    l.core.live_op.set(1);
    l.unload(0);
    // claim the placeholder again (the DSP load of the new version) without publishing
    l.claim_only(0, &f.a);
    let d = vrows();
    assert_eq!(
        (d.row_owner[row], d.row_pub[row]),
        (1, 2),
        "claimed: the DSP holds the disabled placeholder"
    );
    // the steps: word 10 (step ROW_STEPS - 2) carries the mark and the undo that disables the row again; the type write has neither
    let step = |want: u32| {
        let mut w = want;
        unsafe { l.driver.dsp_row_step(&l.core, 0, &mut w, true) }.unwrap()
    };
    let (live, last) = (step(ROW_STEPS - 2), step(ROW_STEPS - 1));
    assert_eq!((live.word % ROW_WORDS, live.value), (10, 0));
    assert!(live.has_undo && live.published.is_some());
    assert_eq!(
        live.undo, ty,
        "the undo of the live step disables the row again"
    );
    assert!(last.last && !last.has_undo && last.published.is_none());
    // a publication that failed after the claim: the teardown parks the row again (a reload) or frees it (not)
    unsafe { l.driver.dsp_clear(&l.core, 0) };
    assert_eq!(vrows().row_owner[row], PLACEHOLDER_OWNER, "parked again");
    unsafe { l.driver.live_done(&l.core) };
    assert_eq!(
        rows_owned_by(&l.core, PLACEHOLDER_OWNER),
        vec![row],
        "the number is in the map: the module is absent and its row stays parked"
    );
}

#[test]
fn a_machine_row_unloaded_in_a_reload_shows_missing_and_plays_no_stock_sound() {
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DN2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    l.core.live_op.set(1);
    l.unload(0);
    let s = &reg().s;
    let text = |b: &[u8]| {
        String::from_utf8_lossy(&b[..b.iter().position(|c| *c == 0).unwrap()]).into_owned()
    };
    assert_eq!(
        (text(&s.mach_name[0]), text(&s.mach_short[0])),
        ("MISSING".to_string(), "MISS".to_string())
    );
    let rep = |b: &[u8]| {
        (
            u32::from_be_bytes(b[0..4].try_into().unwrap()),
            text(&b[12..]),
        )
    };
    assert_eq!(
        rep(&s.rep_long[0]),
        (7, "MISSING".to_string()),
        "the descriptor's long name is a firmware string of the text"
    );
    assert_eq!(rep(&s.rep_short[0]), (4, "MISS".to_string()));
    let inf = unsafe { dkrt_svc_rows::dynreg(&l.core) };
    let row = unsafe {
        dkrt_abi::mem::iw(inf, w::M_ROWS)
            + (DYN_TYPE_FIRST - dkrt_abi::mem::iw(inf, w::M_FIRST))
                * dkrt_abi::mem::iw(inf, w::M_ROW_SIZE)
    };
    assert_eq!(
        unsafe { l.core.rd(row + dkrt_abi::mem::iw(inf, w::O_OWN)) },
        1
    );
}

/// The machine selection view caches the machine list: a reload rebuilds it through `dynreg_info` words 48 (the variable that holds the view) and 49 (the builder), once
/// the module's rows are in the DSP (`on_loaded`), and a load outside a reload does not.
#[test]
fn the_machine_list_is_rebuilt_at_the_end_of_every_reload() {
    use std::cell::RefCell;
    use std::rc::Rc;
    const VAR: u32 = 0x7000_0000;
    const BUILD: u32 = 0x4006_0f50;
    let calls: Rc<RefCell<Vec<(u32, Vec<usize>)>>> = Rc::default();
    let table = |var: u32, build: u32| {
        let mut t = vec![0u32; 64];
        t[w::VIEW_VAR] = var;
        t[w::VIEW_BUILD] = build;
        t
    };
    let run = |words: Vec<u32>, view: u32, phase: u32| {
        let mut core = FakeCore::new();
        core.set_dynreg(words);
        core.mem.borrow_mut().insert(VAR, view);
        core.live_op.set(phase);
        let log = calls.clone();
        *core.on_call.borrow_mut() = Some(Box::new(move |a, args| {
            log.borrow_mut().push((a, args.to_vec()));
            0
        }));
        Provider::reload_done(&mut Machines::new(), &core);
        let got = calls.borrow().clone();
        calls.borrow_mut().clear();
        got
    };
    assert_eq!(
        run(table(VAR, BUILD), 0x1234_5678, 1),
        vec![(BUILD, vec![0x1234_5678])],
        "the builder gets the stored view"
    );
    assert_eq!(
        run(table(VAR, BUILD), 0x1234_5678, 0),
        vec![(BUILD, vec![0x1234_5678])],
        "after a reload that failed too"
    );
    assert!(
        run(table(VAR, BUILD), 0, 1).is_empty(),
        "the view does not exist yet"
    );
    assert!(
        run(table(0, 0), 0x1234_5678, 1).is_empty(),
        "an image without the hook has no view variable or builder"
    );
}

/// The check of a new file that reloads module A (`reload_check` runs the DSP section checks while A is live): A's own voice type and rows are not "used by another module".
/// On the device this was `E_DSP_TYPE` for the reload of any machine with DSP. The phase names the module being reloaded (`CoreApi::live_op`: record index + 1).
#[test]
fn the_dsp_checks_of_a_reload_leave_the_module_being_replaced_out_of_used_by_another_module() {
    use dkrt_abi::dsp::{DspSec, VOICE_ENTRY, VOICE_TYPE, VOICE_V3};
    use dkrt_test_support::contract::locked;
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<MachinesRecipe>>::new(&f);
    assert_eq!(l.load(1, &f.b), dkrt_sdk::OK);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK); // A holds a published voice row
    let sec_of = |keys: &[u32]| {
        let mut bytes = vec![0u8; VOICE_V3 as usize * keys.len()];
        for (i, k) in keys.iter().enumerate() {
            let at = VOICE_V3 as usize * i;
            bytes[at + VOICE_TYPE as usize..at + VOICE_TYPE as usize + 4]
                .copy_from_slice(&k.to_be_bytes());
            bytes[at + VOICE_ENTRY as usize..at + VOICE_ENTRY as usize + 4]
                .copy_from_slice(&0u32.to_be_bytes());
        }
        bytes
    };
    let run = |keys: &[u32], phase: u32, rows: bool| {
        let bytes = sec_of(keys);
        let sec = DspSec {
            s: bytes.as_ptr(),
            version: 5,
            size: bytes.len() as u32,
            image_size: 0x1000,
            image_off: 0,
            reloc_off: 0,
            nreloc: 0,
            import_off: 0,
            nimport: 0,
            voice_off: 0,
            nvoice: keys.len() as u32,
            fx_off: 0,
            nfx: 0,
            vstride: VOICE_V3,
            bank_off: 0,
        };
        l.core.boot_index.set(0); // the module being checked is at selection index 0: `reload_check` sets it as a load does
        l.core.live_op.set(phase);
        let r = unsafe {
            if rows {
                l.driver.dsp_check_rows(&l.core, &sec)
            } else {
                l.driver.dsp_check_types(&l.core, &sec)
            }
        };
        l.core.boot_index.set(u32::MAX);
        l.core.live_op.set(0);
        r
    };
    // the types: another module that takes A's type is refused; the reload of A is not
    assert_eq!(
        run(&[KEY_A], 0, false),
        dkrt_sdk::E_DSP_TYPE,
        "a second module with A's type is used by another module"
    );
    assert_eq!(
        run(&[KEY_A], 1, false),
        dkrt_sdk::OK,
        "the reload of A (owner 1) may use A's type"
    );
    assert_eq!(
        run(&[KEY_A], 2, false),
        dkrt_sdk::E_DSP_TYPE,
        "the reload of B does not make A's type free"
    );
    // the rows: 4 rows, B's and A's, 2 free. Three voices, one of A's type, fit through A's own row in a reload and do not otherwise
    let keys = [KEY_A, 0x00dd_0000, 0x00ee_0000];
    assert_eq!(run(&keys, 1, true), dkrt_sdk::OK, "A's row is taken over");
    assert_eq!(run(&keys, 0, true), dkrt_sdk::E_DSP_ROWS);
    assert_eq!(
        run(&[0x00cc_0000, 0x00dd_0000, 0x00ee_0000], 1, true),
        dkrt_sdk::E_DSP_ROWS,
        "no voice of A's type: no row to take over"
    );
    l.unload(0);
    l.unload(1);
}
