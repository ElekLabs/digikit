//! The service contract suite (`dkrt_test_support::contract`) for the project service on every device with a DSP. The service owns no record kind. A module's input is its
//! settings: with the service loaded the runtime writes them through to the current project (`scope_put`), so the modules below put one value in their `init`, as the runtime does for a
//! `dk_settings_put`. That data is the project's, not the module's: it stays after the module unloads, until the project goes. The fixture declares it as `survives`, so the suite
//! leaves it out of "the state is back at its start" and every other lifecycle check applies; the current slot (`state`) must come back.
//! The module that must be refused makes a record of a kind no service owns, which the loader refuses at the commit.
//!
//! The case the plan names for the live job (a project change reloads the modules whose settings changed) is a rule of the service's `install` (tests/service.rs) and of the
//! runtime's `settings_reload`; the suite has no hot-reload fixture.
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::profiles::ALL;
use dkrt_svc_project::dkp;
use dkrt_svc_project::engine::{
    Env, ListPoll, ListRow, LiveJob, LoadedModule, ModuleId, ModulePath, ModuleWarning, Pending,
    ProjectService, SlotRecord,
};
use dkrt_svc_project::ProjectSvc;
use dkrt_test_support::contract::{contract, Dsp, Fixture, Init, Module, Rows};
use dkrt_test_support::FakeCore;
use std::sync::Mutex;

/// The fixture's service state is a static: the profiles run one at a time.
static LOCK: Mutex<()> = Mutex::new(());
static STATE: Mutex<Option<Box<ProjectService>>> = Mutex::new(None);

/// An environment with one project in the firmware list and no store: enough to make a project current.
struct Quiet;
impl Env for Quiet {
    fn device(&self) -> u16 {
        1
    }
    fn live_op(&self) -> u32 {
        0
    }
    fn now_ms(&self) -> u32 {
        0
    }
    fn irq_mask(&self) -> u32 {
        0
    }
    fn irq_restore(&self, _: u32) {}
    fn slot_record(&self, slot: u32) -> Option<SlotRecord> {
        (slot == 3).then(|| SlotRecord {
            name: *b"CONTRACT\0\0\0\0\0\0\0\0",
            stamp: 7,
        })
    }
    fn list_start(&mut self) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn list_next(&mut self, _: &mut ListRow) -> ListPoll {
        ListPoll::End
    }
    fn read(&mut self, _: &[u8], _: &mut [u8]) -> i32 {
        dkrt_sdk::E_NOT_FOUND
    }
    fn post(&mut self) -> i32 {
        dkrt_sdk::OK
    }
    fn log(&mut self, _: u16, _: [u32; 5]) {}
    fn toast(&mut self, _: u32) {}
    fn record_get(&mut self, _: &[u8], _: &mut [u8]) -> i32 {
        dkrt_sdk::proto::E_SET_NOKEY
    }
    fn changed(&mut self, _: &[u8]) {}
    fn loaded_module(&self, _: usize) -> Option<LoadedModule> {
        None
    }
    fn store_hash(&self, _: &[u8]) -> u32 {
        0
    }
    fn find_module_file(&mut self, _: &[u8]) -> Result<ModulePath, i32> {
        Err(dkrt_sdk::E_NOT_FOUND)
    }
    fn live_load(&mut self, _: &ModulePath) -> i32 {
        dkrt_sdk::E_BUSY
    }
    fn live_job(&self) -> LiveJob {
        LiveJob {
            phase: 0,
            result: 0,
            op: 0,
        }
    }
    fn selection_add(&mut self, _: &ModulePath) -> i32 {
        0
    }
    fn warn_module(&mut self, _: ModuleWarning, _: &ModuleId) {}
}

/// What the runtime does for the module's `dk_settings_put`: the current project takes the value.
fn put(owner: &[u8], value: &[u8]) {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        g.as_mut()
            .expect("a project is current")
            .scope_put(owner, b"k", value),
        0
    );
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

fn keys() -> KeysCopy {
    KeysCopy::empty()
}

fn module(sel: u32, init: fn(&mut Init)) -> Module {
    Module {
        sel,
        keys,
        groups: &[],
        init,
        dsp: Dsp::default(),
    }
}

/// The service's own view that must come back: the current slot.
fn state(_: &FakeCore) -> Vec<u32> {
    let g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    vec![g.as_ref().expect("a project is current").slot() as u32]
}

/// What stays on purpose: the status word, the bytes in use and a sum of the image.
fn survives(_: &FakeCore) -> Vec<u32> {
    let g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let p = g.as_ref().expect("a project is current");
    let used = dkp::head_of(&p.live_image().0).used as usize;
    vec![
        p.status(),
        used as u32,
        p.live_image().0[..used].iter().map(|b| *b as u32).sum(),
    ]
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
fn the_project_service_passes_the_contract_on_every_device_with_a_dsp() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // a project is current before the suite runs
    let mut p = Box::new(ProjectService::new());
    let none = Pending::new();
    let mut env = Quiet;
    p.tick(
        &mut env,
        &Pending {
            slot_val: 3,
            slot_flag: 1,
            ..Pending::new()
        },
    );
    for _ in 0..4 {
        p.tick(&mut env, &none);
    }
    assert_eq!(p.slot(), 3);
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    for p in ALL.iter().filter(|p| p.dyn_types > 0) {
        contract::<ProjectSvc>(&fixture(p.id));
    }
}
