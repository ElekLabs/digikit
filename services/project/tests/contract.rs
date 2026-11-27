//! The service contract suite (`provider_contract`) for the project owner on every device with a DSP. The service owns no record kind. A module's input is its
//! settings: with the service loaded the runtime writes them through to the current project (`scope_put`), so the modules below put one value in their `init`, as the runtime does for a
//! `dk_settings_put`. That data is the project's, not the module's: it stays after the module unloads, until the project goes. The fixture declares it as `survives`, so the suite
//! leaves it out of "the state is back at its start" and every other lifecycle check applies; the current slot (`state`) must come back.
//! The module that must be refused makes a record of a kind no service owns, which the loader refuses at the commit.
//!
//! The case the plan names for the live job (a project change reloads the modules whose settings changed) is a rule of the service's `install` (tests/service/) and of the
//! runtime's `settings_reload`; the suite has no hot-reload fixture.
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_svc_project::dkp;
use dkrt_svc_project::engine::{Pending, StoreJob};
use dkrt_svc_project::project::Project;
use dkrt_test_support::contract::{Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::{provider_contract, subject, FakeCore, Recipe, RowBuilder};

mod host;
use host::TestHost;

type Owner = Project<TestHost>;

/// The owner with the lifecycle and the state part: the service has no record kind, no DSP, no query.
struct ProjectRecipe;

impl Recipe for ProjectRecipe {
    type P = Owner;
    const NAME: &'static str = "project";
    const NONE_STARTS_DIFFERENTLY: Option<&'static str> = Some(
        "import(None) starts named and unbound: a fresh start at run time would bind the firmware's slot byte, which is right only at boot",
    );
    fn parts(b: RowBuilder<Owner>) -> RowBuilder<Owner> {
        b.state()
    }
    fn change_state(p: &mut Owner) -> bool {
        current(p);
        p.scope_put(b"test/ca", b"k", b"unsaved") == 0
    }
}

/// A project is current (slot 3): what the stock firmware's project list gives the tick. Once; later calls find it current.
fn current(p: &mut Owner) {
    if p.engine().slot() == 3 {
        return;
    }
    let job = StoreJob::new();
    p.tick(
        &Pending {
            slot_val: 3,
            slot_flag: 1,
            ..Pending::new()
        },
        &job,
    );
    for _ in 0..4 {
        p.tick(&Pending::new(), &job);
    }
    assert_eq!(p.engine().slot(), 3);
}

/// What the runtime does for the module's `dk_settings_put`: the current project takes the value.
fn put(owner: &[u8], value: &[u8]) {
    subject::<Owner, _>(|p| {
        current(p);
        assert_eq!(p.scope_put(owner, b"k", value), 0);
    });
}
fn a(_: &mut Init) {
    put(b"test/ca", b"first");
}
/// The owner of `a` with another value.
fn a2(_: &mut Init) {
    put(b"test/ca", b"second");
}
fn b(_: &mut Init) {
    put(b"test/cb", b"other");
}
/// Refused at the commit, before it could put anything.
fn stray_record(i: &mut Init) {
    i.record(0x7fff, &[0; 8]);
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

/// The service's own view that must come back: the current slot (made current by the first look, before any module loads).
fn state(_: &FakeCore) -> Vec<u32> {
    subject::<Owner, _>(|p| {
        current(p);
        vec![p.engine().slot() as u32]
    })
}

/// What stays on purpose: the status word, the bytes in use and a sum of the image.
fn survives(_: &FakeCore) -> Vec<u32> {
    subject::<Owner, _>(|p| {
        current(p);
        let used = dkp::head_of(&p.engine().live_image().0).used as usize;
        vec![
            p.status(),
            used as u32,
            p.engine().live_image().0[..used]
                .iter()
                .map(|b| *b as u32)
                .sum(),
        ]
    })
}

fn fixture(device: u32) -> Fixture {
    Fixture {
        name: "project",
        device,
        kinds: &[],
        image: |_| {},
        state,
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
        stateless: false,
        salt_same: &[
            ("memory", "the service writes no firmware word"),
            ("dsp", "the service has no DSP section"),
            ("settings", "a load writes no setting"),
            (
                "state",
                "the current slot is the same whatever module loads",
            ),
        ],
        bad: module(2, stray_record),
        // none: the service has no `apply` and no limit of its own at the commit
        bad_late: None,
        rules: false,
        parked_knobs: None,
        survives: Some(survives),
    }
}

#[test]
fn the_project_owner_passes_the_contract_on_every_device_with_a_dsp() {
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        provider_contract::<ProjectRecipe>(&fixture(p.id));
    }
}
