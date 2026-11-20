//! The service contract suite (`dkrt_test_support::contract`) for the shortcuts service on every profile. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/shortcut-demo`: its `init` makes one `dk_shortcut_add` call.
use core::ffi::c_void;
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_sdk::records::KINDS_SHORTCUTS;
use dkrt_svc_shortcuts::bindings::Callback;
use dkrt_svc_shortcuts::build::{shortcut_add, ShortcutDef};
use dkrt_svc_shortcuts::records::{state, ShortcutsService};
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::FakeCore;
use std::sync::Mutex;

/// The binding table is a static of the crate: the profile loop runs one device at a time here.
static LOCK: Mutex<()> = Mutex::new(());

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
    unsafe { shortcut_add(init.core, init.ctx(), &d as *const ShortcutDef as *const u8) };
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
    let t = state().table();
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
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for p in ALL.iter() {
        contract::<ShortcutsService>(&fixture(p.id));
    }
}
