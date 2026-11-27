//! The provider contract over the fixture `local-ping` (an owner object with the lifecycle, tick and query parts) on `FakeCore`: the shared suite (`provider_contract`) on every
//! profile, and what only this fixture shows, through the part tables of its row as the runtime calls them: the counters of attach and detach per consumer, the tick, the
//! query and the stop.
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_fixture_local_ping::{LocalPing, COUNTERS_LEN, OP_COUNT, OP_COUNTERS, STATE_VERSION};
use dkrt_sdk::{E_NOT_FOUND, E_TOO_BIG};
use dkrt_test_support::contract::{Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::{provider_contract, Driver, FakeCore, ProviderDriver, Recipe, RowBuilder};

/// The parts the file adopts (`service!(LocalPing, "local-ping", &[], parts = [Tick, Query, State], ...)`).
struct PingRecipe;

impl Recipe for PingRecipe {
    type P = LocalPing;
    const NAME: &'static str = "local-ping";
    fn parts(b: RowBuilder<LocalPing>) -> RowBuilder<LocalPing> {
        b.tick().query().state()
    }
}

fn nothing(_: &mut Init) {}

/// A module the service must refuse at its commit: it emits a record of a kind nobody owns, which the suite's collector refuses.
fn unowned_record(init: &mut Init) {
    init.record(0x7fff, &[0; 4]);
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
        name: "local-ping",
        device,
        kinds: &[],
        image: |_| {},
        state: |_| Vec::new(),
        a: module(0, nothing),
        b: module(1, nothing),
        a2: module(0, nothing),
        enable: &[],
        parks_rows: false,
        kept: |_, _, _| false,
        kept_dsp: |_, _, _| false,
        rows: Rows::NONE,
        absent_rows: None,
        live: None,
        survives: None,
        stateless: true,
        salt_same: &[
            ("memory", "the fixture writes no firmware memory"),
            ("dsp", "the fixture has no DSP section"),
            ("settings", "a load writes no setting"),
            ("state", "the fixture keeps no table for modules"),
        ],
        bad: module(2, unowned_record),
        bad_late: None,
        rules: false,
        parked_knobs: None,
    }
}

#[test]
fn local_ping_passes_the_contract_on_every_profile() {
    for p in ALL.iter() {
        provider_contract::<PingRecipe>(&fixture(p.id));
    }
}

fn counters(d: &ProviderDriver<PingRecipe>) -> (u32, u32, u32) {
    let mut reply = [0u8; COUNTERS_LEN];
    let n = unsafe { d.handle().query(OP_COUNTERS, &[], &mut reply) };
    assert_eq!(n, COUNTERS_LEN as i32);
    let word = |i: usize| u32::from_be_bytes([reply[i * 4], reply[i * 4 + 1], reply[i * 4 + 2], reply[i * 4 + 3]]);
    (word(0), word(1), word(2))
}

#[test]
fn every_consumer_that_comes_up_is_counted_with_or_without_records_and_each_one_that_goes_is_counted_apart() {
    let core = FakeCore::new();
    let d = <ProviderDriver<PingRecipe> as Driver>::start(&core);
    assert_eq!(counters(&d), (0, 0, 0), "a fresh instance counts nothing");
    unsafe {
        // owner 1 comes without records, owner 2 with a section that has none of this service's kinds
        d.attach_without_records(&core, 1);
        let section = [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let c = dkrt_abi::service::ApplyCtx { owner: 2, reg: section.as_ptr(), base: 0x4000 };
        assert_eq!(d.apply(&core, &c), dkrt_sdk::OK);
        assert_eq!(counters(&d), (2, 0, 0));
        d.with_instance(|p| assert_eq!(p.last_attached(), 2));
        d.remove(&core, 2);
        assert_eq!(counters(&d), (2, 1, 0));
        // a detach of a consumer that never came up is counted too: `detach` is idempotent and the service keeps no per-consumer state here
        d.remove(&core, 1);
        d.remove(&core, 1);
        assert_eq!(counters(&d), (2, 3, 0));
    }
}

#[test]
fn the_tick_counts_in_every_phase_and_the_query_answers_what_the_instance_holds() {
    let core = FakeCore::new();
    let d = <ProviderDriver<PingRecipe> as Driver>::start(&core);
    unsafe {
        for phase in [0, 1, 2] {
            d.tick(&core, phase);
        }
    }
    d.with_instance(|p| assert_eq!(p.ticks(), 3));
    assert_eq!(counters(&d), (0, 0, 3));
    // an unknown operation and a reply buffer that is too short are refused with their own statuses
    let mut reply = [0u8; COUNTERS_LEN];
    assert_eq!(unsafe { d.handle().query(OP_COUNT + 1, &[], &mut reply) }, E_NOT_FOUND);
    assert_eq!(unsafe { d.handle().query(OP_COUNTERS, &[], &mut reply[..COUNTERS_LEN - 1]) }, E_TOO_BIG);
}

#[test]
fn the_service_may_stop_and_its_instance_is_taken_by_stop() {
    let core = FakeCore::new();
    let d = <ProviderDriver<PingRecipe> as Driver>::start(&core);
    assert_eq!(unsafe { d.teardown_refusal(&core, 1) }, dkrt_sdk::OK, "the default `can_stop` accepts");
    unsafe { d.attach_without_records(&core, 1) };
    unsafe { d.remove(&core, 1) };
    d.stop(&core);
}

#[test]
fn the_count_of_consumers_survives_through_the_state_part_and_a_start_without_bytes_counts_from_zero() {
    let core = FakeCore::new();
    let old = <ProviderDriver<PingRecipe> as Driver>::start(&core);
    unsafe {
        old.attach_without_records(&core, 1);
        old.attach_without_records(&core, 2);
        old.remove(&core, 1);
    }
    let c = &core as *const FakeCore as *const core::ffi::c_void;
    let mut bytes = [0u8; 4];
    assert_eq!(unsafe { old.handle().state_export_max(c) }, 4);
    assert_eq!(unsafe { old.handle().state_export(c, &mut bytes) }, 4);
    assert_eq!(bytes, 2u32.to_be_bytes(), "the count is the attaches, whatever the detaches did");
    old.stop(&core);

    let new = <ProviderDriver<PingRecipe> as Driver>::start(&core);
    assert_eq!(new.handle().state_versions(), Some((STATE_VERSION, STATE_VERSION)));
    assert_eq!(unsafe { new.handle().state_import(c, Some((STATE_VERSION, &bytes))) }, dkrt_sdk::OK);
    unsafe { new.attach_without_records(&core, 1) };
    let mut reply = [0u8; 4];
    assert_eq!(unsafe { new.handle().query(OP_COUNT, &[], &mut reply) }, 4);
    assert_eq!(u32::from_be_bytes(reply), 3, "the carried 2 and the attach of the new copy");
    // bytes of the wrong length are refused (the service's status), and import(None) starts again from zero
    assert_eq!(unsafe { new.handle().state_import(c, Some((STATE_VERSION, &bytes[..3]))) }, dkrt_sdk::E_RANGE);
    assert_eq!(unsafe { new.handle().state_import(c, None) }, dkrt_sdk::OK);
    new.with_instance(|p| assert_eq!(p.count(), 0));
}
