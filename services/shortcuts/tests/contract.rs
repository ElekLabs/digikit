//! The service contract suite (`dkrt_test_support::contract`) for the shortcuts service on every profile. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/shortcut-demo`: its `init` makes one `dk_shortcut_add` call.
use core::ffi::c_void;
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_sdk::records::KINDS_SHORTCUTS;
use dkrt_svc_shortcuts::bindings::Callback;
use dkrt_svc_shortcuts::builder::ShortcutDef;
use dkrt_svc_shortcuts::shortcuts::Shortcuts;
use dkrt_test_support::contract::{Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::{provider_contract, subject, FakeCore, Recipe, RowBuilder};

// the core imports of the file's code (no runtime is linked)
mod stubs;

// the service file's source: its `service!` call is the one place that lists the parts
#[path = "../dks/src/lib.rs"]
pub mod file;
pub use file::{builder, DK_SVC_OWNER};

/// The parts the file adopts, read from the row its `service!` call declares.
struct ShortcutsRecipe;

impl Recipe for ShortcutsRecipe {
    type P = Shortcuts;
    const NAME: &'static str = "shortcuts";
    fn parts(b: RowBuilder<Shortcuts>) -> RowBuilder<Shortcuts> {
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
                && !has(PART_DESCRIBE, DESCRIBE_NEWEST),
            "the recipe has no mapping for a part of the row"
        );
        let b = if has(PART_RECORDS, RECORDS_NEWEST) {
            b.records(&KINDS_SHORTCUTS)
        } else {
            b
        };
        if has(PART_TICK, TICK_NEWEST) {
            b.tick()
        } else {
            b
        }
    }
}

/// The logical keys of the fixtures: the generated numbers of `[keys] order` (sdk/sdk.toml), as `<dk/gen/keys.h>` has them.
use dkrt_sdk::keys::{NO, TRK, YES};
/// The suite loads a module at 0x4000 with an image of 0x4000 bytes.
const BASE: usize = 0x4000;

fn code(offset: usize) -> Option<Callback> {
    // SAFETY: a fake address, only compared and stored
    Some(unsafe { core::mem::transmute::<usize, Callback>(BASE + offset) })
}

fn shortcut(
    init: &mut Init,
    id: &'static [u8],
    label: &'static [u8],
    keys: [u32; 2],
    on: Option<Callback>,
    arg: usize,
) {
    let d = ShortcutDef {
        id: id.as_ptr(),
        label: label.as_ptr(),
        keys,
        on,
        off: code(0x240),
        arg: arg as *mut c_void,
    };
    // the builder call of the module's `init`, on the owner that the suite's driver started
    let (core, ctx) = (init.core, init.ctx());
    subject::<Shortcuts, _>(|s| unsafe {
        s.builder()
            .add(core, ctx, &d as *const ShortcutDef as *const u8)
    });
}

fn a(init: &mut Init) {
    shortcut(
        init,
        b"hint\0",
        b"TRACK NAME HINT\0",
        [TRK, YES],
        code(0x200),
        0x10,
    );
}
fn b(init: &mut Init) {
    shortcut(init, b"other\0", b"OTHER\0", [TRK, NO], code(0x220), 0x20);
}
/// The keys of `a` with other texts and callbacks.
fn a2(init: &mut Init) {
    shortcut(
        init,
        b"hint2\0",
        b"ANOTHER HINT\0",
        [TRK, YES],
        code(0x280),
        0x30,
    );
}
/// No `on` callback: the commit refuses it.
fn bad(init: &mut Init) {
    shortcut(init, b"bad\0", b"BAD\0", [TRK, YES], None, 0);
}

/// The table: its size, and for each binding the owner, the keys, whether it is disabled, the first four characters of id and label, the argument and the callbacks.
fn table(_core: &FakeCore) -> Vec<u32> {
    subject::<Shortcuts, _>(|s| {
        let t = s.table();
        let mut v = vec![t.len() as u32];
        for i in 0..dkrt_svc_shortcuts::bindings::MAX_BINDINGS {
            if let Some(b) = t.get(i) {
                v.extend([
                    u32::from(b.owner),
                    u32::from(b.keys[0]),
                    u32::from(b.keys[1]),
                    u32::from(b.disabled),
                    u32::from_be_bytes([b.id[0], b.id[1], b.id[2], b.id[3]]),
                    u32::from_be_bytes([b.label[0], b.label[1], b.label[2], b.label[3]]),
                    b.arg as usize as u32,
                    b.on.map_or(0, |f| f as *const () as usize as u32),
                    b.off.map_or(0, |f| f as *const () as usize as u32),
                ]);
            }
        }
        v
    })
}

fn module(sel: u32, init: fn(&mut Init)) -> Module {
    Module {
        sel,
        keys: KeysCopy::empty,
        groups: &[],
        init,
        dsp: Dsp::default(),
    }
}

fn fixture(device: u32) -> Fixture {
    Fixture {
        name: "shortcut-demo",
        device,
        kinds: &KINDS_SHORTCUTS,
        image: |_| {},
        state: table,
        a: module(0, a),
        b: module(1, b),
        a2: module(0, a2),
        enable: &[],
        parks_rows: false,
        kept: |_, _, _| false,
        kept_dsp: |_, _, _| false,
        rows: Rows::NONE,
        absent_rows: None,
        live: None,
        survives: None,
        stateless: false,
        salt_same: &[
            ("memory", "a shortcut writes no firmware memory"),
            ("dsp", "a shortcut has no DSP section"),
            ("settings", "a load writes no setting"),
        ],
        bad: module(2, bad),
        // none: `check` refuses a full table, and `apply_record` has no other limit
        bad_late: None,
        rules: true,
        parked_knobs: None,
    }
}

#[test]
fn the_shortcuts_service_passes_the_contract_on_every_profile() {
    for p in ALL.iter() {
        provider_contract::<ShortcutsRecipe>(&fixture(p.id));
    }
}

/// The queued `on` and `off` calls only run in the UI pass, and the commit and apply only through the records part: the file must adopt both.
#[test]
fn the_file_adopts_the_records_and_tick_parts() {
    use dkrt_abi::provider::{
        check, PartLookup, PART_RECORDS, PART_TICK, RECORDS_NEWEST, TICK_NEWEST,
    };
    // SAFETY: the row is a static of this binary
    let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
    for (tag, newest) in [(PART_RECORDS, RECORDS_NEWEST), (PART_TICK, TICK_NEWEST)] {
        assert!(
            matches!(row.part(tag, newest), PartLookup::Found { .. }),
            "part {tag}"
        );
    }
}
