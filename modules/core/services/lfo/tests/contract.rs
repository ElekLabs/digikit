//! The service contract suite (`dkrt_test_support::contract`) for the lfo service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/lfo-shape`: its `init` makes one `dk_record_add` of an `lfo_shape` record (the shape id is the module's key, then a ramp of 32 samples).
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_sdk::records::{KINDS_LFO, KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_svc_lfo::graph;
use dkrt_svc_lfo::records::{name_of, number_of, owner_of, shape, used, Lfo, SAMPLES};
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Module};
use dkrt_test_support::FakeCore;
use std::sync::Mutex;

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

/// The table of shapes: how many, who owns the numbers the keys of the modules got, the name and the first and last samples of what is kept.
fn state(_core: &FakeCore) -> Vec<u32> {
    let mut v = vec![used() as u32];
    for key in [SHAPE_A, SHAPE_B] {
        // modules `a` and `b` are at selection indexes 0 and 1
        let num = number_of(if key == SHAPE_A { 0 } else { 1 }, key);
        v.push(num);
        v.push(owner_of(num) as u32);
        v.push(name_of(num).map_or(0, |n| u32::from_be_bytes(n)));
        v.push(shape(num).map_or(0xffff, |s| {
            s[0] as u16 as u32 | (s[SAMPLES - 1] as u16 as u32) << 16
        }));
    }
    // the pictures the page draws for the shapes
    v.push(graph::digest());
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
        image: |_| {},
        state,
        a: module(0, &[(KIND_LFO_SHAPE, &[SHAPE_A])], a),
        b: module(1, &[(KIND_LFO_SHAPE, &[SHAPE_B])], b),
        a2: module(0, &[(KIND_LFO_SHAPE, &[SHAPE_A])], a2),
        enable: &[],
        kept: |_, _, _| false,
        kept_dsp: |_, _| false,
        live: None,
        salt_same: &[
            ("memory", "the shape table is the service's own state, shown by the state view; no firmware word"),
            ("dsp", "an LFO shape has no DSP section"),
            ("settings", "a load writes no setting"),
        ],
        bad: module(2, &[(KIND_LFO_SHAPE, &[SHAPE_BAD])], bad),
        // none: `check` refuses a table that is too full or an id kept already, and `apply_record` has no other limit
        bad_late: None,
    }
}

#[test]
fn the_lfo_service_passes_the_contract_on_every_device_with_a_dsp() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        contract::<Lfo>(&fixture(p.id));
    }
}
