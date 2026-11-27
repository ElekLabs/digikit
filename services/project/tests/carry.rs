//! The state a reload of the service file carries (`State` of the owner): the same view, status, dirty flag, epoch and image after a round trip, the hooks' record that lives
//! outside the engine, the start of a project that is named and unbound when nothing (or something of no layout of ours) is carried, and `can_stop` while work is in flight.
use core::mem::MaybeUninit;
use dkrt_abi::provider::{Provider, State};
use dkrt_sdk::proto::E_SET_OFF;
use dkrt_sdk::E_BUSY;
use dkrt_svc_project::engine::{JobSpec, Pending, CARRY_MAX, CARRY_VERSION, ST_DIRTY};
use dkrt_svc_project::project::{Host, Project};
use dkrt_test_support::FakeCore;

mod host;
use host::{TestHost, POST_STATUS};

type Owner = Project<TestHost>;

fn start(core: &FakeCore) -> Box<MaybeUninit<Owner>> {
    let mut slot = Box::new(MaybeUninit::<Owner>::uninit());
    <Owner as Provider<FakeCore>>::start_in(core, &mut slot).expect("start");
    slot
}

/// A project of slot 3 is current with a value of the project scope put (so the image is dirty).
fn edited(p: &mut Owner) {
    let job = p.host().job();
    p.tick(
        &Pending {
            slot_val: 3,
            slot_flag: 1,
            ..Pending::new()
        },
        job,
    );
    for _ in 0..4 {
        p.tick(&Pending::new(), job);
    }
    assert_eq!(p.engine().slot(), 3);
    assert_eq!(p.scope_put(b"test/ca", b"k", b"unsaved"), 0);
}

fn export(p: &mut Owner, core: &FakeCore) -> Vec<u8> {
    let mut out = vec![0u8; State::export_max(p, core)];
    let n = State::export(p, core, &mut out);
    out.truncate(n);
    out
}

#[test]
fn a_round_trip_keeps_the_view_status_dirty_epoch_and_image() {
    let core = FakeCore::new();
    let (mut old, mut new) = (start(&core), start(&core));
    // SAFETY: `start_in` made both
    let (old, new) = unsafe { (old.assume_init_mut(), new.assume_init_mut()) };
    edited(old);
    assert_ne!(old.status() & ST_DIRTY, 0);
    let bytes = export(old, &core);
    assert!(!bytes.is_empty() && bytes.len() <= CARRY_MAX);
    assert_eq!(State::import(new, &core, Some(&bytes)), Ok(()));
    let (a, b) = (old.engine(), new.engine());
    assert_eq!(a.slot(), b.slot());
    assert_eq!(a.status(), b.status());
    assert_eq!(a.current(), b.current());
    assert_eq!(a.epoch(), b.epoch());
    assert!(
        a.live_image().0 == b.live_image().0,
        "the image is the same bytes"
    );
    let mut value = [0u8; 16];
    let n = new.scope_get(b"test/ca", b"k", &mut value);
    assert_eq!(
        &value[..n as usize],
        b"unsaved",
        "the unsaved edit is read back"
    );
    assert_eq!(
        export(new, &core),
        bytes,
        "export after import is the same bytes"
    );
}

#[test]
fn the_hooks_record_is_carried_with_the_state() {
    let core = FakeCore::new();
    let (mut old, mut new) = (start(&core), start(&core));
    // SAFETY: as above
    let (old, new) = unsafe { (old.assume_init_mut(), new.assume_init_mut()) };
    edited(old);
    let record = Pending {
        save_slot: 3,
        save_flag: 1,
        clear: [1; 16],
        ..Pending::new()
    };
    let hooks = old.host().hook_record();
    hooks.put_back(&TestHost, &record);
    let bytes = export(old, &core);
    // the new copy's record is empty until the import puts the old one in
    hooks.put_back(&TestHost, &Pending::new());
    State::import(new, &core, Some(&bytes)).unwrap();
    let got = new.host().hook_record();
    assert_eq!((got.save_slot, got.save_flag), (3, 1));
    assert_eq!(got.clear, [1; 16]);
    assert_eq!(got.slot_flag, 0);
}

#[test]
fn nothing_or_bytes_of_no_layout_of_ours_start_named_and_unbound() {
    let core = FakeCore::new();
    for from in [None, Some(&[1u8, 2, 3][..]), Some(&[][..])] {
        let mut slot = start(&core);
        // SAFETY: as above
        let p = unsafe { slot.assume_init_mut() };
        edited(p);
        State::import(p, &core, from).unwrap();
        assert_eq!(p.engine().slot(), -1, "{from:?}");

        assert_eq!(p.status(), 0);
        assert!(p.current().is_err());
        assert_eq!(
            p.scope_put(b"test/ca", b"k", b"v"),
            E_SET_OFF,
            "module writes go to the settings record"
        );
        // named: no bind from the firmware's slot byte
        let job = p.host().job();
        p.tick(&Pending::new(), job);
        assert_eq!(p.engine().slot(), -1);
    }
}

#[test]
fn can_stop_is_busy_while_a_stage_runs_or_a_job_is_posted() {
    let core = FakeCore::new();
    let mut slot = start(&core);
    // SAFETY: as above
    let p = unsafe { slot.assume_init_mut() };
    assert_eq!(Provider::can_stop(p, &core), Ok(()));
    // the setter named a project: the next pass lists its files
    let job = p.host().job();
    p.tick(
        &Pending {
            slot_val: 3,
            slot_flag: 1,
            ..Pending::new()
        },
        job,
    );
    assert_eq!(Provider::can_stop(p, &core), Err(E_BUSY));
    for _ in 0..8 {
        p.tick(&Pending::new(), job);
    }
    assert_eq!(Provider::can_stop(p, &core), Ok(()), "the work ended");
    POST_STATUS.with(|s| s.set(0));
    let mut spec = JobSpec::new();
    spec.write(
        dkrt_svc_project::name::Path::from_bytes(b"projects/1-A.dkp").unwrap(),
        3,
    );
    assert!(job.post(&mut TestHost, spec));
    assert_eq!(
        Provider::can_stop(p, &core),
        Err(E_BUSY),
        "a store job is posted"
    );
}

#[test]
fn the_layout_version_is_the_one_the_part_declares() {
    assert_eq!(<Owner as State<FakeCore>>::VERSION, CARRY_VERSION);
    assert!(<Owner as State<FakeCore>>::ACCEPTS_FROM <= CARRY_VERSION);
}
