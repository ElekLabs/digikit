//! The service contract suite (`dkrt_test_support::contract`) for the machines service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/sine` (`src/module.c`: one machine with a page of one parameter) and a second machine with another key: each makes the builder calls through
//! the service's `build::*`. The image tables are those of the device profiles (`Profile` of `tests/build.rs`) as `core/dynreg` writes them, over the fake core's memory.
use core::ptr::addr_of;
use dkneeds::keys::name_hash;
use dkrt_abi::dsp::{FREE_TYPE, I_AREA_BYTES, I_VFIRST, I_VFREE, ROW_WORDS, WIN_VOICES};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::{DN2, DT2};
use dkrt_sdk::limits;
use dkrt_sdk::records::KINDS_MACHINES;
use dkrt_svc_machines::build;
use dkrt_svc_machines::records::{owned_ids, rstate, Machines};
use dkrt_svc_params::dynreg::w;
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Late, Module};
use dkrt_test_support::FakeCore;

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
/// - machine row (`0x30_0000`, 0x100 bytes): the name (0) and the short name (0x4c) are the empty firmware string; the long and short text (4, 8) are the service's
///   empty representation; word 0x30 after the eight knob codes (`O_F0 + 4 * KNOBS`) is the track level code 10; the position in the list (0x44) is not written by
///   `remove` and is no order in a hidden row (any value);
/// - names table (`0x40_0000`, 12 bytes per type), the types of the module rows: tab and title are the empty string;
/// - parameter registry row (`0x20_0000`, 0x20 bytes): name and short are the empty string, the template (8) is `RESERVE_TEMPLATE`, the slot (0xc) is `SLOT_BASE`;
/// - Digitone II page row (`0x60_0000`, 0x80 bytes): name and title are the empty representation, the level code after the eight knob codes (0x28) is 10, the machine
///   (0x70) is the free machine `PG_FREE` (0xff).
fn kept(_core: &FakeCore, addr: u32, value: u32) -> bool {
    const EMPTY: u32 = 0x50_0000;
    let rep = addr_of!(rstate().empty_rep) as u32 + 12;
    let in_rows = |base: u32, size: u32, n: usize| {
        (addr >= base && ((addr - base) / size) < n as u32).then(|| (addr - base) % size)
    };
    if let Some(off) = in_rows(0x30_0000, 0x100, limits::MACHINE_ROWS) {
        return match off {
            0 | 0x4c => value == EMPTY,
            4 | 8 => value == rep,
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
        return matches!(off, 0 | 4) && value == EMPTY;
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
    let mut d = vec![0u32; 16];
    d[I_AREA_BYTES] = 0x10_0000;
    d[I_VFIRST] = 2;
    d[I_VFREE] = 4;
    core.dsp_table = d;
    // `machines_info`: the adapter's native page register is bound (a call of it is a firmware call the fake answers)
    let mut mi = vec![0u32; dkrt_svc_machines::info::WORDS];
    mi[dkrt_svc_machines::info::w::MAGIC] = dkrt_svc_machines::info::MAGIC;
    mi[dkrt_svc_machines::info::w::VERSION] = dkrt_svc_machines::info::VERSION;
    mi[dkrt_svc_machines::info::w::N_REGISTER] = 0x6000_0010;
    mi[dkrt_svc_machines::info::w::N_REGISTER_V2] = 0x6000_0020;
    core.info_tables.push((name_hash("machines_info"), mi));
}

/// The service's own view: who owns the machine, parameter and page rows, and the type numbers each owner has.
fn state(core: &FakeCore) -> Vec<u32> {
    let s = rstate();
    let mut v: Vec<u32> = s.mach_owner.iter().map(|o| *o as u32).collect();
    v.extend(s.param_owner.iter().map(|o| *o as u32));
    v.extend(s.page_owner.iter().map(|o| *o as u32));
    for owner in 1..=3u8 {
        let mut ids = [0u8; 8];
        let n = owned_ids(core, owner, &mut ids);
        v.push(n);
        v.extend(ids.iter().take(n as usize).map(|i| *i as u32));
    }
    v
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
        kept,
        kept_dsp: |_, _| false,
        // the machines service queues no live value for its rows
        live: None,
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
    }
}

#[test]
fn the_machines_service_passes_the_contract_on_the_dt2() {
    contract::<Machines>(&fixture(DT2.id));
}

#[test]
fn the_machines_service_passes_the_contract_on_the_dn2() {
    contract::<Machines>(&fixture(DN2.id));
}
