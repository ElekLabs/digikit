//! The service contract suite (`dkrt_test_support::contract`) for the machines service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/sine` (`src/module.c`: one machine with a page of one parameter) and a second machine with another key: each makes the builder calls through
//! the service's `build::*`. The image tables are those of the device profiles (`Profile` of `tests/build.rs`) as `core/dynreg` writes them, over the fake core's memory.
use core::ptr::addr_of;
use dkneeds::keys::name_hash;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{FREE_TYPE, I_AREA_BYTES, I_VFIRST, I_VFREE, ROW_WORDS, WIN_VOICES};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::FwMem;
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
        rules: true,
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
    let first = unsafe { dkrt_abi::mem::iw(core.dsp_info(), I_VFIRST) };
    link_word(core, WIN_VOICES, ROW_WORDS * (first + row as u32) + word)
}

fn rows_owned_by(core: &FakeCore, owner: u8) -> Vec<usize> {
    (0..dkrt_abi::dsp::MAX_ROWS)
        .filter(|k| core.dsp().row_owner[*k] == owner)
        .collect()
}

#[test]
fn a_voice_row_unloaded_in_a_reload_is_a_disabled_placeholder_that_keeps_its_type() {
    use dkrt_abi::dsp::{PLACEHOLDER_OWNER, PLACEHOLDER_PC};
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    for dev in [DT2.id, DN2.id] {
        let f = fixture(dev);
        let l = Lifecycle::<Machines>::new(&f);
        assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
        let row = rows_owned_by(&l.core, 1)[0];
        let ty = l.core.dsp().row_type[row];
        assert_eq!(
            voice_row_word(&l.core, row, 0),
            Some(ty),
            "the row is published"
        );
        l.core.reload_phase.set(1);
        l.unload(0);
        let d = l.core.dsp();
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
        // a placeholder row counts in the CAPACITY figure (`capacity::used_rows` counts every non-zero owner): the device test reads that figure against the row masks
        let used = |l: &dkrt_test_support::contract::Lifecycle<Machines>| {
            unsafe {
                <Machines as dkrt_abi::service::Service<FakeCore>>::cap(
                    &l.core,
                    dkrt_sdk::proto::CAPK_VOICE_ROWS,
                )
            }
            .unwrap()
            .1
        };
        assert_eq!(
            (used(&l), l.core.dsp().mod_rows[0]),
            (1, 0),
            "a parked row is in use and in no module's row mask"
        );
        // the reload ended without the module: the row is free, in the DSP too
        unsafe { <Machines as dkrt_abi::service::Service<FakeCore>>::reload_done(&l.core) };
        let d = l.core.dsp();
        assert_eq!((d.row_owner[row], d.row_type[row]), (0, FREE_TYPE));
        assert_eq!(used(&l), 0);
        assert_eq!(voice_row_word(&l.core, row, 0), Some(FREE_TYPE));
        assert_eq!(voice_row_word(&l.core, row, 10), Some(0));
    }
}

#[test]
fn an_unload_outside_a_reload_frees_the_voice_row_and_leaves_a_gap_row_as_it_did() {
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Machines>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    l.unload(0); // the boot's failure path (`undo_early`) is this
    let d = l.core.dsp();
    assert_eq!(
        (d.row_owner[row], d.row_type[row]),
        (0, FREE_TYPE),
        "a free row, no placeholder"
    );
    assert_eq!(voice_row_word(&l.core, row, 0), Some(FREE_TYPE));
    assert!(rows_owned_by(&l.core, dkrt_abi::dsp::PLACEHOLDER_OWNER).is_empty());
    // the machine row is the gap row: empty names, own_dsp 0
    let s = &rstate().s;
    assert_ne!(s.mach_name[0][0], b'M', "no MISSING text");
    let inf = unsafe { dkrt_svc_params::dynreg(&l.core) };
    let row = unsafe {
        dkrt_abi::mem::iw(inf, w::M_ROWS)
            + (DYN_TYPE_FIRST - dkrt_abi::mem::iw(inf, w::M_FIRST))
                * dkrt_abi::mem::iw(inf, w::M_ROW_SIZE)
    };
    assert_eq!(
        unsafe { l.core.rd(row + dkrt_abi::mem::iw(inf, w::O_OWN)) },
        0
    );
}

#[test]
fn the_module_of_the_same_number_takes_its_placeholder_row_and_another_module_leaves_it() {
    use dkrt_abi::dsp::PLACEHOLDER_OWNER;
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Machines>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    l.core.reload_phase.set(1);
    l.unload(0);
    // module B (another key) loads: A's placeholder stays, B has its own row
    assert_eq!(l.load(1, &f.b), dkrt_sdk::OK);
    assert_eq!(
        l.core.dsp().row_owner[row],
        PLACEHOLDER_OWNER,
        "B did not take A's placeholder"
    );
    assert!(!rows_owned_by(&l.core, 2).contains(&row));
    // A loads again (its number is not refused for the placeholder): it takes its row, and the disabled word is cleared
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let d = l.core.dsp();
    assert_eq!(d.row_owner[row], 1, "A's row is the one it had");
    assert_eq!(
        voice_row_word(&l.core, row, 10),
        Some(0),
        "the disabled word is cleared when the row is published"
    );
    assert_eq!(voice_row_word(&l.core, row, 0), Some(d.row_type[row]));
    // nothing is left to free when the reload ends
    unsafe { <Machines as dkrt_abi::service::Service<FakeCore>>::reload_done(&l.core) };
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
    l.core.reload_phase.set(0);
    l.unload(0);
    l.unload(1);
}

#[test]
fn many_types_loaded_and_reloaded_do_not_use_up_the_voice_rows() {
    use dkrt_abi::service::Service;
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Machines>::new(&f);
    let used = |l: &Lifecycle<Machines>| {
        unsafe { <Machines as Service<FakeCore>>::cap(&l.core, dkrt_sdk::proto::CAPK_VOICE_ROWS) }
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
            l.core.reload_phase.set(1);
            l.unload(rec);
            assert_eq!(
                l.load(rec, if rec == 0 { &f.a } else { &f.b }),
                dkrt_sdk::OK
            );
            unsafe { <Machines as Service<FakeCore>>::reload_done(&l.core) };
            l.core.reload_phase.set(0);
        }
        assert_eq!(
            used(&l),
            2,
            "round {round}: two voice rows, no placeholder left"
        );
        l.unload(0);
        l.unload(1);
        assert_eq!(used(&l), 0, "round {round}");
    }
}

#[test]
fn a_voice_whose_type_has_a_placeholder_needs_no_free_row() {
    use dkrt_abi::dsp::{DspSec, VOICE_TYPE, VOICE_V3};
    use dkrt_abi::service::Service;
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Machines>::new(&f);
    assert_eq!(l.load(1, &f.b), dkrt_sdk::OK); // one row
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK); // one row, which the reload parks
    l.core.reload_phase.set(1);
    l.unload(0);
    // 4 rows: B's, A's placeholder, 2 free. A file with 3 voices fits through the placeholder when one of them has A's type, and not when none has
    let check = |keys: [u32; 3]| {
        let mut bytes = vec![0u8; VOICE_V3 as usize * 3];
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
            nvoice: 3,
            fx_off: 0,
            nfx: 0,
            vstride: VOICE_V3,
            bank_off: 0,
        };
        l.core.boot_index.set(0);
        let r = unsafe { <Machines as Service<FakeCore>>::dsp_check_rows(&l.core, &sec) };
        l.core.boot_index.set(u32::MAX);
        r
    };
    assert_eq!(check([KEY_A, 0x00dd_0000, 0x00ee_0000]), dkrt_sdk::OK);
    assert_eq!(
        check([0x00cc_0000, 0x00dd_0000, 0x00ee_0000]),
        dkrt_sdk::E_DSP_ROWS
    );
    unsafe { <Machines as Service<FakeCore>>::reload_done(&l.core) };
    l.core.reload_phase.set(0);
    l.unload(1);
}

#[test]
fn a_failed_publication_into_a_reused_placeholder_row_disables_it_again() {
    use dkrt_abi::dsp::{PLACEHOLDER_OWNER, ROW_STEPS, ROW_WORDS};
    use dkrt_abi::service::Service;
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Machines>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let row = rows_owned_by(&l.core, 1)[0];
    let ty = l.core.dsp().row_type[row];
    l.core.reload_phase.set(1);
    l.unload(0);
    // claim the placeholder again (the DSP load of the new version) without publishing
    l.claim_only(0, &f.a);
    let d = l.core.dsp();
    assert_eq!(
        (d.row_owner[row], d.row_pub[row]),
        (1, 2),
        "claimed: the DSP holds the disabled placeholder"
    );
    // the steps: word 10 (step ROW_STEPS - 2) carries the mark and the undo that disables the row again; the type write has neither
    let step = |want: u32| {
        let mut w = want;
        unsafe { <Machines as Service<FakeCore>>::dsp_row_step(&l.core, 0, &mut w, true) }.unwrap()
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
    unsafe { <Machines as Service<FakeCore>>::dsp_clear(&l.core, 0) };
    assert_eq!(
        l.core.dsp().row_owner[row],
        PLACEHOLDER_OWNER,
        "parked again"
    );
    unsafe { <Machines as Service<FakeCore>>::reload_done(&l.core) };
    assert!(rows_owned_by(&l.core, PLACEHOLDER_OWNER).is_empty());
}

#[test]
fn a_machine_row_unloaded_in_a_reload_shows_missing_and_plays_no_stock_sound() {
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DN2.id);
    let l = Lifecycle::<Machines>::new(&f);
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    l.core.reload_phase.set(1);
    l.unload(0);
    let s = &rstate().s;
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
    let inf = unsafe { dkrt_svc_params::dynreg(&l.core) };
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
    use dkrt_abi::service::Service;
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
        core.reload_phase.set(phase);
        let log = calls.clone();
        *core.on_call.borrow_mut() = Some(Box::new(move |a, args| {
            log.borrow_mut().push((a, args.to_vec()));
            0
        }));
        unsafe { <Machines as Service<FakeCore>>::reload_done(&core) };
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
/// On the device this was `E_DSP_TYPE` for the reload of any machine with DSP. The phase names the module being reloaded (`CoreApi::reload_phase`: record index + 1).
#[test]
fn the_dsp_checks_of_a_reload_leave_the_module_being_replaced_out_of_used_by_another_module() {
    use dkrt_abi::dsp::{DspSec, VOICE_ENTRY, VOICE_TYPE, VOICE_V3};
    use dkrt_abi::service::Service;
    use dkrt_test_support::contract::{locked, Lifecycle};
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Machines>::new(&f);
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
        l.core.reload_phase.set(phase);
        let r = unsafe {
            if rows {
                <Machines as Service<FakeCore>>::dsp_check_rows(&l.core, &sec)
            } else {
                <Machines as Service<FakeCore>>::dsp_check_types(&l.core, &sec)
            }
        };
        l.core.boot_index.set(u32::MAX);
        l.core.reload_phase.set(0);
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
