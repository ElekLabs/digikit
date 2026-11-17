//! The service contract suite (`dkrt_test_support::contract`) for the machines service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/sine` (`src/module.c`: one machine with a page of one parameter) and a second machine with another key: each makes the builder calls through
//! the service's `build::*`. The image tables are those of the device profiles (`Profile` of `tests/build.rs`) as `core/dynreg` writes them, over the fake core's memory.
use dkrt_abi::dsp::{I_AREA_BYTES, I_VFIRST, I_VFREE};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::{DN2, DT2};
use dkrt_sdk::limits;
use dkrt_sdk::records::KINDS_MACHINES;
use dkrt_svc_machines::build;
use dkrt_svc_machines::records::{owned_ids, rstate, Machines};
use dkrt_svc_params::dynreg::w;
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Module};
use dkrt_test_support::FakeCore;

/// The first run-time machine type (the same on both devices of the suite).
const DYN_TYPE_FIRST: u32 = DT2.dyn_type_first;

/// The machine keys of the keys tables of the two modules (`hash24 << 8`).
const KEY_A: u32 = 0x00a1_0100;
const KEY_B: u32 = 0x00b2_0200;

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
    t[w::P_CLONE] = 0;
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
    // `dyndsp_info`: the voice rows start at row 2 of the DSP's table and four are free
    let mut d = vec![0u32; 16];
    d[I_AREA_BYTES] = 0x10_0000;
    d[I_VFIRST] = 2;
    d[I_VFREE] = 4;
    core.dsp_table = d;
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

fn module(sel: u32, keys: fn() -> KeysCopy, init: fn(&mut Init)) -> Module {
    Module {
        sel,
        keys,
        init,
        dsp: Dsp {
            voices: 1,
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
        a: module(0, keys_a, sine),
        b: module(1, keys_b, sine_two),
        bad: module(2, keys_none, sine),
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
