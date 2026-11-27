//! The service contract suite (`dkrt_test_support::contract`) for the lfo service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/lfo-shape`: its `init` makes one `dk_record_add` of an `lfo_shape` record (the shape id is the module's key, then a ramp of 32 samples).
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::FwMem;
use dkrt_abi::profiles::ALL;
use dkrt_sdk::records::{KINDS_LFO, KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_svc_lfo::graph;
use dkrt_svc_lfo::records::SAMPLES;
use dkrt_svc_lfo::shapes::{name_of, owner_of, shape, used};
use dkrt_svc_lfo::wave::set_firmware;
use dkrt_test_support::contract::{Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::{provider_contract, subject, FakeCore, Recipe, RowBuilder};
use file::image_table::host;
use std::sync::Mutex;

// the core imports of the file's code (no runtime is linked)
// the core imports (`dkc_*`) of the service file, as host functions (this test links no runtime)
#[path = "../../../runtime/core/test-support/stubs/core_imports.rs"]
mod stubs;

// the service file's source: its `service!` call is the one place that lists the parts
#[path = "../dks/src/lib.rs"]
pub mod file;
pub use file::DK_SVC_OWNER;

/// The parts the file adopts, read from the row its `service!` call declares.
struct LfoRecipe;

impl Recipe for LfoRecipe {
    type P = file::Owner;
    const NAME: &'static str = "lfo";
    fn parts(b: RowBuilder<file::Owner>) -> RowBuilder<file::Owner> {
        use dkrt_abi::provider::{
            check, PartLookup, DESCRIBE_NEWEST, DSP_NEWEST, PART_DESCRIBE, PART_DSP, PART_QUERY,
            PART_RECORDS, PART_TICK, QUERY_NEWEST, RECORDS_NEWEST, TICK_NEWEST,
        };
        // SAFETY: the row is a static of this binary
        let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
        let has = |tag, newest| matches!(row.part(tag, newest), PartLookup::Found { .. });
        // a part this recipe cannot build (the service does not implement it) must not be in the row
        assert!(
            !has(PART_DSP, DSP_NEWEST)
                && !has(PART_QUERY, QUERY_NEWEST)
                && !has(PART_DESCRIBE, DESCRIBE_NEWEST)
                && !has(PART_TICK, TICK_NEWEST),
            "the recipe has no mapping for a part of the row"
        );
        if has(PART_RECORDS, RECORDS_NEWEST) {
            b.records(&KINDS_LFO)
        } else {
            b
        }
    }
}

/// The shape table and the graph slots are statics of the crate: the two profile tests (and the other test files, each its own process) run one at a time here.
static LOCK: Mutex<()> = Mutex::new(());

/// The keys that stand for `DK_KEY_SHAPE` of three modules (the key derives from the module id: `hash24 << 8`).
const SHAPE_A: u32 = 0x00a1_0100;
const SHAPE_B: u32 = 0x00b2_0200;
const SHAPE_BAD: u32 = 0x00c3_0300;

/// `tests/fixtures/modules/lfo-shape/src/module.c`: the shape record with `key`, the name RAMP and a ramp from `from` in steps of 2048 (`from` is at most -30721, so the last sample fits i16).
fn lfo_shape(init: &mut Init, key: u32, from: i16) {
    let mut body = key.to_be_bytes().to_vec();
    body.extend_from_slice(b"RAMP");
    for i in 0..SAMPLES as i32 {
        body.extend_from_slice(&((from as i32 + 2048 * i) as i16).to_be_bytes());
    }
    assert_eq!(body.len() + 8, LEN_LFO_SHAPE as usize);
    init.record(KIND_LFO_SHAPE, &body);
}

fn a(init: &mut Init) {
    lfo_shape(init, SHAPE_A, -31744);
}
fn b(init: &mut Init) {
    lfo_shape(init, SHAPE_B, -31000);
}
/// The shape key of `a` with other samples.
fn a2(init: &mut Init) {
    lfo_shape(init, SHAPE_A, -30800);
}
/// A key of 0 is no shape: the service refuses it at the check.
fn bad(init: &mut Init) {
    lfo_shape(init, 0, -31744);
}

/// The fixture image's parameter table: the stock table at `STOCK`, its RAM copy at `COPY` (`P_TABLE`), 60-byte rows, the registry from row 300, and the WAVE rows of the three LFOs
/// (codes 79, 89 and 99) holding 0..0x600 with the default 0x100, 0 and 0 in the copy.
const STOCK: u32 = 0x1000;
const COPY: u32 = 0x8000;
const CODES: [u32; 3] = [79, 89, 99];

fn image(core: &mut FakeCore) {
    let mut t = vec![0u32; 90];
    t[20] = COPY;
    t[27] = 60;
    t[23] = 300;
    core.set_rows_info(t);
    core.set_lfo_table(7, 8, 0x1000, 0x1010);
    host::reset();
    for (c, def) in CODES.iter().zip([0x100, 0, 0]) {
        unsafe {
            core.wr(COPY + 60 * c + 8, 0);
            core.wr(COPY + 60 * c + 12, 0x600);
            core.wr(COPY + 60 * c + 16, def);
        }
    }
    set_firmware(STOCK, CODES.map(|c| STOCK + 60 * c));
}

/// The table of shapes: how many, who owns the numbers the keys of the modules got, the name and the first and last samples of what is kept.
fn state(core: &FakeCore) -> Vec<u32> {
    let mut v = vec![used() as u32];
    for key in [SHAPE_A, SHAPE_B] {
        // modules `a` and `b` are at selection indexes 0 and 1
        let num =
            subject::<file::Owner, _>(|l| l.number_of(if key == SHAPE_A { 0 } else { 1 }, key));
        v.push(num);
        v.push(owner_of(num) as u32);
        v.push(name_of(num).map_or(0, |n| u32::from_be_bytes(n)));
        v.push(shape(num).map_or(0xffff, |s| {
            s[0] as u16 as u32 | (s[SAMPLES - 1] as u16 as u32) << 16
        }));
        // what the image's table plays for the number: the samples of the shape, or nothing (the stock sine)
        v.push(host::playing(num).map_or(0xffff, |s| {
            s[0] as u16 as u32 | (s[SAMPLES - 1] as u16 as u32) << 16
        }));
    }
    // the pictures the page draws for the shapes
    v.push(graph::digest());
    // the maximum of the WAVE rows: the highest loaded number << 8, the stock 0x600 when none is loaded (the contract compares it with the start state in cases 3 and 7)
    for c in CODES {
        v.push(unsafe { core.rd(COPY + 60 * c + 12) });
    }
    v
}

/// The modules list no machine keys; their shape keys are the group of kind `KIND_LFO_SHAPE` of the header table (`CoreApi::keys_of`).
fn keys() -> KeysCopy {
    KeysCopy::empty()
}

fn module(sel: u32, groups: &'static [(u32, &'static [u32])], init: fn(&mut Init)) -> Module {
    Module {
        sel,
        keys,
        groups,
        init,
        dsp: Dsp::default(),
    }
}

fn fixture(device: u32) -> Fixture {
    Fixture {
        name: "lfo-shape",
        device,
        kinds: &KINDS_LFO,
        image,
        state,
        a: module(0, &[(KIND_LFO_SHAPE, &[SHAPE_A])], a),
        b: module(1, &[(KIND_LFO_SHAPE, &[SHAPE_B])], b),
        a2: module(0, &[(KIND_LFO_SHAPE, &[SHAPE_A])], a2),
        enable: &[],
        parks_rows: false,
        kept: |_, _, _| false,
        kept_dsp: |_, _, _| false,
        rows: Rows::NONE,
        absent_rows: None,
        live: None,
        stateless: false,
        salt_same: &[
            ("memory", "the maximum of the WAVE rows is the highest loaded number << 8, and A and a2 have the same key, so the same number"),
            ("dsp", "an LFO shape has no DSP section"),
            ("settings", "a load writes no setting"),
        ],
        bad: module(2, &[(KIND_LFO_SHAPE, &[SHAPE_BAD])], bad),
        // none: `check` refuses a table that is too full or an id kept already, and `Records::apply` has no other limit
        bad_late: None,
        rules: true,
        parked_knobs: None,
        survives: None,
    }
}

/// The file adopts the records part (the shapes reach the table only through it), and no other.
#[test]
fn the_file_adopts_the_records_part() {
    use dkrt_abi::provider::{check, PartLookup, PART_RECORDS, RECORDS_NEWEST};
    // SAFETY: the row is a static of this binary
    let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
    assert!(matches!(
        row.part(PART_RECORDS, RECORDS_NEWEST),
        PartLookup::Found { .. }
    ));
}

#[test]
fn the_lfo_service_passes_the_contract_on_every_device_with_a_dsp() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // the service's firmware addresses are a static: clear them even when the contract panics
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            set_firmware(0, [0; 3]);
        }
    }
    let _reset = Reset;
    host::install();
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        provider_contract::<LfoRecipe>(&fixture(p.id));
    }
}

#[test]
fn the_row_reaches_the_object_through_the_owner_guard() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    host::install();
    file::dk_svc_start().expect("start");
    dkrt_test_support::contract::row_is_guarded(
        file::dk_svc_row(),
        &|f| {
            DK_SVC_OWNER.with(|_| f());
        },
        &|| DK_SVC_OWNER.refused(),
    );
    file::dk_svc_stop();
}

/// The name and graph formatters of the MOD page run in the UI task, also inside a part call: they read the static tables and do not enter the owner.
#[test]
fn an_lfo_page_hook_fired_inside_a_part_call_never_enters_the_owner() {
    use dkrt_test_support::hook_context::HookContext;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    host::install();
    file::dk_svc_start().expect("start");
    let hold = |f: &mut dyn FnMut()| {
        DK_SVC_OWNER.run((), |_| f());
    };
    let refused = || DK_SVC_OWNER.refused();
    let mut buf = [0u8; 8];
    let fired = HookContext::InPartCall.fire(&hold, &refused, &mut || unsafe {
        file::hooks::dk_lfo_name(7 << 8, buf.as_mut_ptr(), 7)
    });
    assert_eq!(fired.result, 0, "no shape is loaded for wave 7");
    assert_eq!(fired.refused, 0, "the hook did not enter the owner");
    file::dk_svc_stop();
}

/// An image without `lfo_table_info` (a stock one, or one patched without core/lfo-table) does not start the file: no shape could be played.
#[test]
fn the_file_does_not_start_on_an_image_without_the_wave_table() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    host::uninstall();
    assert_eq!(file::dk_svc_start().err(), Some(dkrt_sdk::E_NO_REGISTRY));
}
