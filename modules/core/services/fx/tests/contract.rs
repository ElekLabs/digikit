//! The service contract suite (`dkrt_test_support::contract`) for the fx service on both profiles. The call lists below stand for the shared fixtures
//! `tests/fixtures/modules/dyn-fx-gain` and `dyn-fx-gain-b`: each makes the builder calls of its `src/module.c` through the service's `build::*`.
use core::ptr::addr_of;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{
    DspState, FXW_P0, FXW_PC, FX_FREE_PC, FX_ROW_WORDS, F_FIRST, F_FREE, MAX_FX, MIX_ONE, WIN_FX,
};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::FwMem;
use dkrt_abi::profiles::{ALL, DT2};
use dkrt_sdk::records::KINDS_FX;
use dkrt_svc_fx::build::{self, ROUTE_MASTER, ROUTE_SEND};
use dkrt_svc_fx::fxpage::{self, INFO_HASH};
use dkrt_svc_fx::fxsend;
use dkrt_svc_fx::records::Fx;
use dkrt_test_support::contract::{contract, locked, Dsp, Fixture, Init, Late, Lifecycle, Module};
use dkrt_test_support::FakeCore;

/// The effect ids of the keys tables of the two modules (what `DK_FX_ID_GAIN` is the index of).
const ID_A: u32 = 0x0000_1001;
const ID_B: u32 = 0x0000_1002;
/// The first of the four effect ids of the module that is refused while its records apply.
const ID_L1: u32 = 0x0000_1003;
const DK_PERCENT: u32 = 1;

fn c(s: &'static [u8]) -> *const u8 {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr()
}

/// `tests/fixtures/modules/dyn-fx-gain/src/module.c`: the effect `gain` (index 0), its page and the GAIN knob, default 0.5; the effect runs on the master.
fn dyn_fx_gain(i: &mut Init) {
    unsafe {
        let fx = build::add_fx(i.core, i.ctx(), 0, c(b"Test Gain\0"), c(b"GAN\0"));
        let p = build::fx_add_page(i.core, fx, c(b"Test Gain\0"), c(b"GAN\0"));
        build::page_add_fx_param(
            i.core,
            p,
            c(b"Gain\0"),
            c(b"GAIN\0"),
            DK_PERCENT,
            0.0,
            1.27,
            0.5,
        );
    }
}

/// `tests/fixtures/modules/dyn-fx-gain-b/src/module.c`: the same effect with MIX 64 and the default 0.25.
fn dyn_fx_gain_b(i: &mut Init) {
    unsafe {
        let fx = build::add_fx(i.core, i.ctx(), 0, c(b"Test Gain B\0"), c(b"GNB\0"));
        build::fx_mix(i.core, fx, 64);
        let p = build::fx_add_page(i.core, fx, c(b"Test Gain B\0"), c(b"GNB\0"));
        build::page_add_fx_param(
            i.core,
            p,
            c(b"Gain\0"),
            c(b"GAIN\0"),
            DK_PERCENT,
            0.0,
            1.27,
            0.25,
        );
    }
}

/// The effect of `a` for another module key set: the key of `a`, other names, MIX, default and routes (master and send, default send), so the send cells and the
/// page of the effect hold other words.
fn fx_alt(i: &mut Init) {
    unsafe {
        let fx = build::add_fx(i.core, i.ctx(), 0, c(b"Alt Gain\0"), c(b"ALT\0"));
        build::fx_mix(i.core, fx, 32);
        build::fx_routes(i.core, fx, ROUTE_MASTER | ROUTE_SEND, ROUTE_SEND);
        let p = build::fx_add_page(i.core, fx, c(b"Alt Gain\0"), c(b"ALT\0"));
        build::page_add_fx_param(
            i.core,
            p,
            c(b"Level\0"),
            c(b"LVL\0"),
            DK_PERCENT,
            0.0,
            1.27,
            0.75,
        );
    }
}

/// Four effects and four DSP rows, where the image has page rows for three (`image`: `PAGES`): the first three pages are written, the fourth effect finds no page row,
/// which `apply` refuses (`E_PAGE_FULL`).
fn four_effects(i: &mut Init) {
    effects(i, 4);
}

/// The same with three effects: all of them find a page row.
fn three_effects(i: &mut Init) {
    effects(i, 3);
}

/// A module with four DSP rows and `n` effects with a page each.
fn four_rows(init: fn(&mut Init)) -> Module {
    Module {
        dsp: Dsp {
            fxs: 4,
            salt: 1,
            ..Dsp::default()
        },
        ..module(3, keys_four, init, 1)
    }
}

fn effects(i: &mut Init, n: u32) {
    unsafe {
        for n in 0..n {
            let fx = build::add_fx(i.core, i.ctx(), n, c(b"Late Gain\0"), c(b"LAT\0"));
            let p = build::fx_add_page(i.core, fx, c(b"Late Gain\0"), c(b"LAT\0"));
            build::page_add_fx_param(
                i.core,
                p,
                c(b"Gain\0"),
                c(b"GAIN\0"),
                DK_PERCENT,
                0.0,
                1.27,
                0.5,
            );
        }
    }
}

/// The pc word of each of the four effect rows of the image (`image` gives `fx_table` first row 6): the word that skips a row.
const ENABLE: [(u32, u32, u32); 4] = [
    (WIN_FX, FX_ROW_WORDS * 6 + FXW_PC, FX_FREE_PC),
    (WIN_FX, FX_ROW_WORDS * 7 + FXW_PC, FX_FREE_PC),
    (WIN_FX, FX_ROW_WORDS * 8 + FXW_PC, FX_FREE_PC),
    (WIN_FX, FX_ROW_WORDS * 9 + FXW_PC, FX_FREE_PC),
];

/// The memory words that differ from the start after a release, on purpose (each a free value of the service):
/// - the name and title words of a page row (offsets 0 and 4): they point to the service's empty string, so no word of the row points to the effect that left;
/// - the entries of the page list buffer at or past the count in the count cell: the buffer is not cleared, the count says how many entries are in it (the test
///   image has no stock entries, so the cell holds the number of entries).
fn kept(core: &FakeCore, addr: u32, _value: u32) -> bool {
    let rows = addr_of!(fxpage::DKRT_FXP_ROWS) as u32;
    if addr >= rows && addr < rows + 0x2c * MAX_FX as u32 && matches!((addr - rows) % 0x2c, 0 | 4) {
        return true;
    }
    let list = addr_of!(fxpage::DKRT_FXP_LIST_BUF) as u32;
    let count = unsafe { core.rd(addr_of!(fxpage::DKRT_FXP_COUNT) as u32) };
    addr >= list + 4 * count && addr < list + 4 * 8
}

/// The DSP state bytes that differ from the start after a release, on purpose: the MIX word of a released effect row is 1.0f (`MIX_ONE`, the mix of a row without
/// a page; the start state holds 0 because no row was ever released).
fn kept_dsp(at: usize, value: u8) -> bool {
    let mix = core::mem::offset_of!(DspState, fx_mix);
    at >= mix && at < mix + 4 * MAX_FX && value == MIX_ONE.to_le_bytes()[(at - mix) % 4]
}

/// What the effect pages do while an effect runs: a changed knob queues the new value on the link (`link_set`) for the row of the effect. For each row a module
/// owns, one live value on its first parameter word.
const LIVE: Option<fn(&FakeCore)> = Some(|core| unsafe {
    for k in 0..MAX_FX {
        if dkrt_svc_fx::dsp::fx_owner_of(core, k) != 0 {
            let slot =
                dkrt_svc_fx::dsp::fx_slot(core, k, FXW_P0).expect("the image has effect rows");
            core.link_set(slot, 0x3f00_0000);
        }
    }
});

fn keys_of(id: u32) -> KeysCopy {
    let mut k = KeysCopy::empty();
    k.nf = 1;
    k.fk[0] = id;
    k
}
fn keys_a() -> KeysCopy {
    keys_of(ID_A)
}
fn keys_b() -> KeysCopy {
    keys_of(ID_B)
}
fn keys_four() -> KeysCopy {
    let mut k = keys_of(ID_L1);
    k.nf = 4;
    k.fk[1] = ID_L1 + 1;
    k.fk[2] = ID_L1 + 2;
    k.fk[3] = ID_L1 + 3;
    k
}
/// A keys table without the effect's row: `init` names an effect index the table does not have, which the commit refuses.
fn keys_none() -> KeysCopy {
    KeysCopy::empty()
}

/// The words of `fxpages_info` (the version-4 image table the service binds) and `dynreg_info`, and the `dynfx_info` the DSP rows come from.
fn image(core: &mut FakeCore) {
    let mut t = vec![0u32; 64];
    t[0] = 0x4658_5047; // "FXPG"
    t[1] = 4;
    t[2] = core.device;
    t[3] = 0x6000; // page rows
    t[4] = 0x2c; // row size
    t[5] = 14; // the page number of row 0
    t[6] = 3; // rows
    t[8] = 8; // list capacity
    t[12] = 40; // the first parameter code of the effect pages
    t[13] = 8;
    t[14] = 0x14; // the kit's array offset
    t[15] = 32; // values
    t[37] = 70; // the mark's array index
    t[38] = 0x46a7;
    t[39] = 2; // send cells
    t[42] = 90; // the first send parameter code
    t[61] = 13;
    t[63] = 0x80;
    core.info_tables.push((INFO_HASH, t));
    let mut d = vec![0u32; 96];
    d[dkrt_svc_params::dynreg::w::P_TABLE] = 0x10000;
    d[dkrt_svc_params::dynreg::w::PF_SIZE] = 0x40;
    d[dkrt_svc_params::dynreg::w::P_ROWS] = 0x20000;
    d[dkrt_svc_params::dynreg::w::P_ROW_SIZE] = 0x20;
    d[dkrt_svc_params::dynreg::w::P_FIRST] = 0;
    d[dkrt_svc_params::dynreg::w::P_TEMPLATE_OFF] = 8;
    core.set_dynreg(d);
    core.fx_table[F_FIRST] = 6;
    core.fx_table[F_FREE] = 4;
    unsafe { fxpage::unbind() };
}

/// The service's own view: the DSP rows in use, the pages and their owners, the send cells.
fn state(core: &FakeCore) -> Vec<u32> {
    let mut v = unsafe {
        let (reserved, used) = fxpage::counts(core);
        vec![reserved, used, fxsend::used()]
    };
    for k in 0..4 {
        v.push(fxpage::page_owner(k) as u32);
        v.push(unsafe { dkrt_svc_fx::dsp::fx_owner_of(core, k) } as u32);
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
            fxs: 1,
            salt,
            ..Dsp::default()
        },
    }
}

fn fixture(device: u32) -> Fixture {
    Fixture {
        name: "dyn-fx-gain",
        device,
        kinds: &KINDS_FX,
        image,
        state,
        a: module(0, keys_a, dyn_fx_gain, 1),
        b: module(1, keys_b, dyn_fx_gain_b, 3),
        a2: module(0, keys_a, fx_alt, 2),
        enable: &ENABLE,
        disabled: &[],
        kept,
        kept_dsp,
        kept_caps: |_, _, _| false,
        // the route of an effect is a setting of the FX ROUTES menu; a module load never writes one
        live: LIVE,
        salt_same: &[("settings", "a load writes no setting")],
        bad: module(2, keys_none, dyn_fx_gain, 1),
        bad_late: Some(Late {
            bad: four_rows(four_effects),
            same_but_ok: four_rows(three_effects),
            // four DSP rows are all the image has: nothing fits beside B
            beside_b: false,
        }),
        rules: true,
    }
}

#[test]
fn the_fx_service_passes_the_contract_on_every_device_with_a_dsp() {
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        contract::<Fx>(&fixture(p.id));
    }
}

/// The kit hooks read the default raw values of each effect row from `DKRT_FXP_DEFS`: after the unload of an effect the row's defaults are 0, not the defaults of the
/// effect that left (found by the contract suite: `remove` published the table before it cleared the defaults).
#[test]
fn an_unloaded_effect_leaves_no_defaults_in_the_table_of_the_kit_hooks() {
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Fx>::new(&f);
    // the table's words as the firmware sees them: the halfwords the service wrote through the core
    let defs = || -> Vec<i32> {
        let at = unsafe { addr_of!(fxpage::DKRT_FXP_DEFS) } as u32;
        (0..64)
            .map(|i| unsafe { l.core.rd16(at + 2 * i) })
            .collect()
    };
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    assert!(
        defs().iter().any(|d| *d != 0),
        "the effect's defaults are in the table"
    );
    l.unload(0);
    assert!(
        defs().iter().all(|d| *d == 0),
        "the defaults of the unloaded effect are gone"
    );
}

/// After an unload the name and title words of the effect's page row point to the service's empty string, not to the strings of the effect that left.
#[test]
fn an_unloaded_effect_page_row_points_to_the_empty_string() {
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Fx>::new(&f);
    let rows = unsafe { addr_of!(fxpage::DKRT_FXP_ROWS) } as u32;
    let words = || unsafe { (FwMem::rd(&l.core, rows), FwMem::rd(&l.core, rows + 4)) };
    assert_eq!(l.load(0, &f.a), dkrt_sdk::OK);
    let (tab, title) = words();
    let empty = unsafe { addr_of!(fxpage::DKRT_FXP_EMPTY) } as u32 + 12;
    assert!(
        tab != empty && title != empty,
        "a loaded page has its strings"
    );
    l.unload(0);
    assert_eq!(words(), (empty, empty));
    // the string is what `make_rep` writes for "": length 0, capacity 0, refcount 0xffffffff (all big-endian words), then the NUL
    // `make_rep` writes the buffer in the host's memory (the static itself), not through the core
    let at = addr_of!(fxpage::DKRT_FXP_EMPTY) as *const u8;
    assert_eq!(
        at as usize % 4,
        0,
        "make_rep writes words: the buffer is aligned"
    );
    assert_eq!(empty, at as usize as u32 + 12);
    let word = |off: usize| unsafe { dkrt_abi::mem::rd32(at.add(off)) };
    assert_eq!((word(0), word(4), word(8)), (0, 0, 0xffff_ffff));
    assert_eq!(unsafe { *at.add(12) }, 0);
}
