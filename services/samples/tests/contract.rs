//! The service contract suite (`dkrt_test_support::provider_contract`) for the samples service on both profiles. The service owns no record kind and its input is the
//! track-table record of a module's DSP section (version 5), not builder calls: the modules below have such a record, as the sample-playback machine under `modules/machines` has
//! for its `sample_records`, with local ids 1 and 2. The module that must be refused makes a record of a kind no service owns, which the loader refuses at the
//! commit after the DSP rows were claimed, so the suite exercises the release of the track table.
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_svc_samples::samples::Samples;
use dkrt_test_support::contract::{Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::{provider_contract, subject, FakeCore, Recipe, RowBuilder};

// the core imports of the file's code (no runtime is linked)
mod stubs;

// the service file's source: its `service!` call is the one place that lists the parts
#[path = "../dks/src/lib.rs"]
pub mod file;
pub use file::{glue, DK_SVC_OWNER};

/// The parts the file adopts, read from the row its `service!` call declares.
struct SamplesRecipe;

impl Recipe for SamplesRecipe {
    type P = Samples;
    const NAME: &'static str = "samples";
    fn parts(b: RowBuilder<Samples>) -> RowBuilder<Samples> {
        use dkrt_abi::provider::{
            check, PartLookup, DESCRIBE_NEWEST, DSP_NEWEST, PART_DESCRIBE, PART_DSP, PART_QUERY,
            PART_RECORDS, PART_TICK, QUERY_NEWEST, RECORDS_NEWEST, TICK_NEWEST,
        };
        // SAFETY: the row is a static of this binary
        let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
        let has = |tag, newest| matches!(row.part(tag, newest), PartLookup::Found { .. });
        // a part this recipe cannot build (the service does not implement it) must not be in the row
        assert!(
            !has(PART_RECORDS, RECORDS_NEWEST)
                && !has(PART_QUERY, QUERY_NEWEST)
                && !has(PART_DESCRIBE, DESCRIBE_NEWEST),
            "the recipe has no mapping for a part of the row"
        );
        let b = if has(PART_DSP, DSP_NEWEST) {
            b.dsp()
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
        groups: &[],
        init,
        dsp: Dsp {
            bank: Some(local),
            ..Dsp::default()
        },
    }
}

/// The service's own view: which module records hold a track table, and the handle each local id of a record has.
fn state(core: &FakeCore) -> Vec<u32> {
    subject::<Samples, _>(|s| {
        let mut v = Vec::new();
        for rec in 0..4 {
            v.push(s.has_table(rec) as u32);
            core.source.set(rec as u8 + 1);
            for local in 1..=3 {
                v.push(s.tracks(core, local) as u32);
            }
        }
        core.source.set(0);
        v
    })
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
        // the keys of `a` and another local id
        a2: module(0, 3, nothing),
        enable: &[],
        parks_rows: false,
        kept: |_, _, _| false,
        kept_dsp: |_, _, _| false,
        rows: Rows::NONE,
        absent_rows: None,
        live: None,
        stateless: false,
        salt_same: &[
            ("memory", "the track table is the service's own state, shown by the state view; no firmware word"),
            ("dsp", "a track-table record changes no DSP state"),
            ("settings", "a load writes no setting"),
        ],
        bad: module(2, 1, stray_record),
        // none: the service has no `apply`; its only refusals are `dsp_admit` and the checks of the section, before any row is written
        bad_late: None,
        rules: false,
        parked_knobs: None,
        survives: None,
    }
}

#[test]
fn the_samples_service_passes_the_contract_on_every_device_with_a_dsp() {
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        provider_contract::<SamplesRecipe>(&fixture(p.id));
    }
}

/// The track tables are only claimed, loaded and released through the DSP part, and the tracks only step in the UI pass: the file must adopt both.
#[test]
fn the_file_adopts_the_dsp_and_tick_parts() {
    use dkrt_abi::provider::{check, PartLookup, DSP_NEWEST, PART_DSP, PART_TICK, TICK_NEWEST};
    // SAFETY: the row is a static of this binary
    let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
    for (tag, newest) in [(PART_DSP, DSP_NEWEST), (PART_TICK, TICK_NEWEST)] {
        assert!(
            matches!(row.part(tag, newest), PartLookup::Found { .. }),
            "part {tag}"
        );
    }
}
