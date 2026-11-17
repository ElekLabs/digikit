//! The service contract suite (`dkrt_test_support::contract`) for the lfo service on both profiles. The call lists below stand for the shared fixture
//! `tests/fixtures/modules/lfo-shape`: its `init` makes one `dk_record_add` of an `lfo_shape` record (the shape id is the module's key, then a ramp of 32 samples).
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_sdk::records::{KINDS_LFO, KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_svc_lfo::records::{owner_of, shape, used, Lfo, SAMPLES};
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Module};
use dkrt_test_support::FakeCore;

/// The ids that stand for the key `DK_KEY_SHAPE` of three modules (the key derives from the module id; the service reads only that it is not 0).
const SHAPE_A: u32 = 0x00a1_0001;
const SHAPE_B: u32 = 0x00b2_0002;

/// `tests/fixtures/modules/lfo-shape/src/module.c`: the shape record with `id` and a ramp from `from` in steps of 100.
fn lfo_shape(init: &mut Init, id: u32, from: i16) {
    let mut body = id.to_be_bytes().to_vec();
    for i in 0..SAMPLES as i16 {
        body.extend_from_slice(&(from + 100 * i).to_be_bytes());
    }
    assert_eq!(body.len() + 8, LEN_LFO_SHAPE as usize);
    init.record(KIND_LFO_SHAPE, &body);
}

fn a(init: &mut Init) {
    lfo_shape(init, SHAPE_A, -1000);
}
fn b(init: &mut Init) {
    lfo_shape(init, SHAPE_B, 500);
}
/// An id of 0 is no shape: the service refuses it at the check.
fn bad(init: &mut Init) {
    lfo_shape(init, 0, 0);
}

/// The table of shapes: how many, who owns the ids of the modules, and the first samples of what is kept.
fn state(_core: &FakeCore) -> Vec<u32> {
    let mut v = vec![used() as u32];
    for id in [SHAPE_A, SHAPE_B] {
        v.push(owner_of(id) as u32);
        v.push(shape(id).map_or(0xffff, |s| s[0] as u16 as u32 | (s[SAMPLES - 1] as u16 as u32) << 16));
    }
    v
}

fn keys() -> KeysCopy {
    KeysCopy::empty()
}

fn module(init: fn(&mut Init)) -> Module {
    Module {
        keys,
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
        a: module(a),
        b: module(b),
        bad: module(bad),
    }
}

#[test]
fn the_lfo_service_passes_the_contract_on_every_device_with_a_dsp() {
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        contract::<Lfo>(&fixture(p.id));
    }
}
