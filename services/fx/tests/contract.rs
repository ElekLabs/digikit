//! The service contract suite (`dkrt_test_support::contract`) for the fx service on both profiles. The call lists below stand for the shared fixtures
//! `tests/fixtures/modules/dyn-fx-gain` and `dyn-fx-gain-b`: each makes the builder calls of its `src/module.c` through the owner's builder (`build::*` below reaches the owner that the
//! suite's driver started).
//! The parts the file adopts are read from the row its own `service!` call declares (the file's source is compiled in by path).
use core::ptr::addr_of;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{FXW_P0, FXW_PC, FX_FREE_PC, FX_ROW_WORDS, MAX_FX, MIX_ONE, WIN_FX};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::FwMem;
use dkrt_abi::profiles::{ALL, DT2};
use dkrt_sdk::records::KINDS_FX;
use dkrt_svc_fx::builder::{ROUTE_MASTER, ROUTE_SEND};
use dkrt_svc_fx::fx::Fx;
use dkrt_svc_fx::fxpage::{self, DeviceFacts};
use dkrt_svc_fx::fxsend;
use dkrt_svc_fx::rows::{self, FxRows};
use dkrt_test_support::contract::{
    locked, Dsp, Fixture, Init, Late, Lifecycle, Module, ParkedKnobs, Rows,
};
use dkrt_test_support::{provider_contract, subject, FakeCore, Provided, Recipe, RowBuilder};

// the core imports of the file's code (no runtime is linked)
// the core imports (`dkc_*`) of the service file, as host functions (this test links no runtime)
#[path = "../../../runtime/core/test-support/stubs/core_imports.rs"]
mod stubs;

// the service file's source: its `service!` call is the one place that lists the parts
#[path = "../dks/src/lib.rs"]
pub mod file;
pub use file::{builder, DK_SVC_OWNER};

/// The parts the file adopts, read from the row its `service!` call declares.
struct FxRecipe;

impl Recipe for FxRecipe {
    type P = Fx;
    const NAME: &'static str = "fx";
    fn parts(b: RowBuilder<Fx>) -> RowBuilder<Fx> {
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
                && !matches!(row.part(PART_QUERY, QUERY_NEWEST), PartLookup::Found { .. }),
            "the file adopts the four parts this recipe builds and no Query part"
        );
        b.records(&KINDS_FX).dsp().tick().describe()
    }
}

/// The builder calls of a module's `init`, on the owner that the suite's driver started.
mod build {
    use super::*;

    pub unsafe fn add_fx(
        core: &FakeCore,
        ctx: *mut u8,
        index: u32,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        subject::<Fx, _>(|fx| fx.builder().add_fx(core, ctx, index, name, short))
    }

    pub unsafe fn fx_add_page(
        core: &FakeCore,
        f: *mut u8,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        subject::<Fx, _>(|fx| fx.builder().fx_add_page(core, f, name, short))
    }

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn page_add_fx_param(
        core: &FakeCore,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        kind: u32,
        min: f32,
        max: f32,
        def: f32,
    ) -> *mut u8 {
        subject::<Fx, _>(|fx| {
            fx.builder()
                .page_add_fx_param(core, p, name, short, kind, min, max, def)
        })
    }

    pub fn fx_routes(core: &FakeCore, f: *mut u8, allowed: u32, def: u32) {
        subject::<Fx, _>(|fx| fx.builder().fx_routes(core, f, allowed, def))
    }

    pub fn fx_mix(core: &FakeCore, f: *mut u8, def: u32) {
        subject::<Fx, _>(|fx| fx.builder().fx_mix(core, f, def))
    }
}

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

/// The pc word of each of the four effect rows of the image (`image` gives `dynfx_info` first row 6): the word that skips a row.
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

/// The bytes of the service's row table that differ from the start after a release, on purpose: the MIX word of a released effect row is 1.0f (`MIX_ONE`, the mix of a
/// row without a page; the start state holds 0 because no row was ever released).
fn kept_rows(_core: &FakeCore, at: usize, value: u8) -> bool {
    let mix = core::mem::offset_of!(FxRows, fx_mix);
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

/// The facts of the test device: 3 page rows, 32 values (the value index of word `j` of effect row `k` is `8k + j`), 2 send cells; `kit` is the address of the kit pointer cell.
fn facts(kit: u32) -> DeviceFacts {
    let mut value_indices = [0u8; 32];
    for (i, v) in value_indices.iter_mut().enumerate() {
        *v = i as u8;
    }
    DeviceFacts {
        device: 1,
        page_first: 14,
        slots: 3,
        array_off: 0x14,
        values_n: 32,
        kit_ptr: kit,
        mark_index: 70,
        send_cells: 2,
        knob_block: 2,
        send_block: 3,
        track_page: 13,
        store_off: 0x80,
        value_indices,
        ..Default::default()
    }
}

/// `rows_info` (the parameter table, the blocks of the effect knobs from code 40 and of the send knobs from code 90), the `dynfx_info` the DSP rows come from, and the table of the
/// effect pages bound from the test device's facts.
fn image(core: &mut FakeCore) {
    let mut d = vec![0u32; 110];
    d[dkrt_svc_rows::rows_info::w::P_TABLE] = 0x10000;
    d[dkrt_svc_rows::rows_info::w::PF_SIZE] = 0x40;
    d[dkrt_svc_rows::rows_info::w::P_ROWS] = 0x20000;
    d[dkrt_svc_rows::rows_info::w::P_ROW_SIZE] = 0x20;
    d[dkrt_svc_rows::rows_info::w::P_FIRST] = 0;
    d[dkrt_svc_rows::rows_info::w::P_TEMPLATE_OFF] = 8;
    core.set_rows_info(d);
    core.set_rows_blocks(&[(2, 40, 24, 0x75), (3, 90, 2, 0x75)]);
    core.set_dynfx(6, 4);
    unsafe { assert_eq!(fxpage::bind_facts(core, &facts(0), [0, 0]), dkrt_sdk::OK) };
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
        parks_rows: false,
        kept,
        // the fx rows are the service's own table, not the loader's `DspState`: nothing of the loader's state is kept
        kept_dsp: |_, _, _| false,
        rows: Rows {
            bytes: || rows::bytes().to_vec(),
            reset: rows::reset,
            kept: kept_rows,
            ..Rows::NONE
        },
        absent_rows: None,
        // the route of an effect is a setting of the FX ROUTING popup; a module load never writes one
        live: LIVE,
        stateless: false,
        salt_same: &[("settings", "a load writes no setting")],
        bad: module(2, keys_none, dyn_fx_gain, 1),
        bad_late: Some(Late {
            bad: four_rows(four_effects),
            same_but_ok: four_rows(three_effects),
            // four DSP rows are all the image has: nothing fits beside B
            beside_b: false,
        }),
        rules: true,
        parked_knobs: Some(parked_knobs()),
        survives: None,
    }
}

#[test]
fn the_fx_service_passes_the_contract_on_every_device_with_a_dsp() {
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        provider_contract::<FxRecipe>(&fixture(p.id));
    }
}

/// The kit hooks read the default raw values of each effect row from `DKRT_FXP_DEFS`: after the unload of an effect the row's defaults are 0, not the defaults of the
/// effect that left (found by the contract suite: `remove` published the table before it cleared the defaults).
#[test]
fn an_unloaded_effect_leaves_no_defaults_in_the_table_of_the_kit_hooks() {
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<FxRecipe>>::new(&f);
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
    let l = Lifecycle::<Provided<FxRecipe>>::new(&f);
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

const KIT_CELL: u32 = 0x7000;
const KIT_A: u32 = 0x4000_1000;
const KIT_B: u32 = 0x4000_3000;

/// The live kit in the image: the cell `KIT_CELL` is the kit pointer cell of the effect pages, and word `j` of effect row `k` lives at array index `8k + j`.
fn with_kit(core: &mut FakeCore) {
    unsafe {
        assert_eq!(
            fxpage::bind_facts(core, &facts(KIT_CELL), [0, 0]),
            dkrt_sdk::OK
        )
    };
}

/// `image` with a live kit.
fn image_with_kit(core: &mut FakeCore) {
    image(core);
    with_kit(core);
}

/// A live kit: the first effect word's index, the kit array, and the mark the pages put in it.
fn kit_array(kit: u32) -> u32 {
    kit + 0x14
}

/// The hooks of the contract's case 13: the knob of effect row 0 in the live kit (`KIT_CELL` holds kit `KIT_A` or `KIT_B`), and the firmware's kit-load count.
fn parked_knobs() -> ParkedKnobs {
    ParkedKnobs {
        image: with_kit,
        kit: |core, n| unsafe { core.wr(KIT_CELL, if n == 0 { KIT_A } else { KIT_B }) },
        get: |core| unsafe { core.rd16(kit_array(core.rd(KIT_CELL))) },
        set: |core, v| unsafe { core.wr16(kit_array(core.rd(KIT_CELL)), v) },
        kit_loaded: |_| unsafe {
            let n = core::ptr::addr_of_mut!(fxpage::DKRT_FXP_NLOAD);
            core::ptr::write_volatile(n, core::ptr::read_volatile(n) + 1);
        },
        kit_reset: || unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(fxpage::DKRT_FXP_NLOAD), 0)
        },
    }
}

/// An effect with a send route that is reloaded gets its send cell back with the module (the unload gives the cell and the page row back, the load takes them again).
/// Docs: docs/plans/extension-model.md, E5b.
#[test]
fn a_reloaded_effect_gets_its_send_cell_and_page_row_back() {
    let _g = locked();
    let f = fixture(DT2.id);
    let l = Lifecycle::<Provided<FxRecipe>>::new(&f);
    unsafe {
        // the effect with the default route "send": a send cell
        assert_eq!(l.load(0, &f.a2), dkrt_sdk::OK);
        assert_eq!(fxsend::used(), 1, "the effect has its send cell");
        l.core.live_op.set(1);
        assert_eq!(
            l.driver.with_instance(
                |fx| <Fx as dkrt_abi::provider::Provider<FakeCore>>::can_stop(fx, &l.core)
            ),
            Ok(()),
            "an effect with a send route is torn down"
        );
        l.unload(0);
        assert_eq!(fxsend::used(), 0, "the cell is given back with the module");
        assert_eq!(fxpage::page_owner(0), 0);
        l.core.live_op.set(0);
        assert_eq!(l.load(0, &f.a2), dkrt_sdk::OK);
        assert_eq!(fxsend::used(), 1, "the route comes back");
        assert_eq!(fxpage::page_owner(0), 1);
        l.unload(0);
    }
}

/// The parked values are stamped for each row with the module's name, the effect id and the kit (the contract's case 13 covers the module and kit parts): the removal
/// of another module's effect later does not refresh the stamp of a row that was parked before a kit was loaded.
#[test]
fn the_removal_of_another_effect_does_not_refresh_the_stamp_of_a_parked_row() {
    let _g = locked();
    let mut f = fixture(DT2.id);
    f.image = image_with_kit;
    let l = Lifecycle::<Provided<FxRecipe>>::new(&f);
    let k = parked_knobs();
    unsafe {
        (k.kit)(&l.core, 0);
        assert_eq!(l.load(0, &f.a2), dkrt_sdk::OK);
        let default = (k.get)(&l.core);
        assert_eq!(l.load(1, &f.b), dkrt_sdk::OK);
        (k.set)(&l.core, 0x1234);
        l.unload(0);
        (k.kit_loaded)(&l.core);
        l.unload(1);
        assert_eq!(l.load(0, &f.a2), dkrt_sdk::OK);
        assert_eq!((k.get)(&l.core), default, "a kit was loaded after the row was parked: the removal of another effect does not refresh its stamp");
        l.unload(0);
        (k.kit_reset)();
    }
}
