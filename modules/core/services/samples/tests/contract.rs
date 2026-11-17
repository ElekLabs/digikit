//! The service contract suite (`dkrt_test_support::contract`) for the samples service on both profiles. The service owns no record kind and its input is the
//! track-table record of a module's DSP section (version 5), not builder calls: the modules below have such a record, as the sample-playback machine under `modules/machines` has
//! for its `sample_records`, with local ids 1 and 2. The module that must be refused makes a record of a kind no service owns, which the loader refuses at the
//! commit after the DSP rows were claimed, so the suite exercises the release of the track table.
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_svc_samples::records::Samples;
use dkrt_svc_samples::tracks;
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Module};
use dkrt_test_support::FakeCore;

fn nothing(_: &mut Init) {}

/// A record of a kind that no service owns: `dk_record_add` refuses it (`E_FORMAT`), and the commit reports the refusal.
fn stray_record(i: &mut Init) {
    i.record(0x7fff, &[0; 8]);
}

fn keys() -> KeysCopy {
    KeysCopy::empty()
}

fn module(sel: u32, local: u32, init: fn(&mut Init)) -> Module {
    Module {
        sel,
        keys,
        init,
        dsp: Dsp {
            bank: Some(local),
            ..Dsp::default()
        },
    }
}

/// The service's own view: which module records hold a track table.
fn state(_core: &FakeCore) -> Vec<u32> {
    (0..4)
        .map(|rec| unsafe { tracks::has(rec) } as u32)
        .collect()
}

fn fixture(device: u32) -> Fixture {
    Fixture {
        name: "sample track table",
        device,
        kinds: &[],
        image: |_| {},
        state,
        a: module(0, 1, nothing),
        b: module(1, 2, nothing),
        bad: module(2, 1, stray_record),
    }
}

#[test]
fn the_samples_service_passes_the_contract_on_every_device_with_a_dsp() {
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        contract::<Samples>(&fixture(p.id));
    }
}
