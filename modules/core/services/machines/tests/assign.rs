//! The sticky machine type numbers over a `FakeCore`: the plan of `settle` from the keys of the selection, the stored map (settings key, migration of the selection
//! record's old bytes) and the answer of `resolve` for the module being loaded. The assignment rules themselves are tested in `dkrt_sdk::sticky`.
use dkneeds::KIND_MACHINE;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::{Service, SettleCtx};
use dkrt_sdk::sel::MAP_N;
use dkrt_sdk::{E_BUSY, OK};
use dkrt_svc_machines::assign::{map_used, resolve, SETTING};
use dkrt_svc_machines::records::Machines;
use dkrt_test_support::{Call, FakeCore};
use std::sync::Mutex;
use std::vec::Vec;

/// The tests drive the statics of the crate (`DKRT_ASSIGN`).
static LOCK: Mutex<()> = Mutex::new(());

const A: u32 = 0x00aa_00;
const B: u32 = 0x00bb_00;
const FIRST: u32 = dkrt_abi::profiles::DT2.dyn_type_first;

/// A core of the DT2 (the run-time machine types come from the device's profile).
fn dt2_core() -> FakeCore {
    let mut c = FakeCore::new();
    c.device = dkrt_abi::profiles::DT2.id;
    c
}

/// A selection of modules, each with its machine keys.
fn selection(core: &FakeCore, modules: &[&[u32]]) -> SettleCtx {
    let mut s = SettleCtx::new();
    s.count = modules.len();
    for (k, keys) in modules.iter().enumerate() {
        core.set_keys(k as u32, KIND_MACHINE, keys);
    }
    s
}

fn stored(core: &FakeCore, map: [u32; MAP_N]) {
    let raw: Vec<u8> = map.iter().flat_map(|w| w.to_be_bytes()).collect();
    core.settings.borrow_mut().push((SETTING.to_vec(), raw));
}

/// The bytes the selection record kept before the service had its settings key.
fn set_legacy(core: &FakeCore, map: [u32; MAP_N]) {
    let mut b = [0u8; 32];
    for (i, w) in map.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
    }
    core.legacy.set(b);
}

fn saved(core: &FakeCore) -> Vec<[u32; MAP_N]> {
    core.calls()
        .iter()
        .filter_map(|c| match c {
            Call::SettingsPut(k, v) if k == SETTING => {
                let mut map = [0u32; MAP_N];
                for (i, w) in v.chunks(4).enumerate() {
                    map[i] = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
                }
                Some(map)
            }
            _ => None,
        })
        .collect()
}

fn settle(core: &FakeCore, sel: &SettleCtx) {
    unsafe { <Machines as Service<FakeCore>>::settle(core, sel) }
}

#[test]
fn a_boot_without_a_map_gives_the_lowest_numbers_in_load_order_and_saves_the_map() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dt2_core();
    settle(&core, &selection(&core, &[&[A, B]]));
    core.boot_index.set(0);
    unsafe {
        assert_eq!((resolve(&core, A), resolve(&core, B)), (FIRST, FIRST + 1));
    }
    assert_eq!(map_used(), 2);
    let mut want = [0u32; MAP_N];
    want[0] = A | FIRST;
    want[1] = B | (FIRST + 1);
    assert_eq!(saved(&core), [want], "the map is saved once");
}

#[test]
fn a_stored_map_keeps_a_machine_on_its_number_and_a_boot_that_changes_nothing_saves_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dt2_core();
    let mut map = [0u32; MAP_N];
    map[0] = B | (FIRST + 5);
    stored(&core, map);
    // the old location holds another number for A: the settings key is the map
    let mut old = [0u32; MAP_N];
    old[0] = A | (FIRST + 2);
    set_legacy(&core, old);
    settle(&core, &selection(&core, &[&[B]]));
    core.boot_index.set(0);
    unsafe {
        assert_eq!(resolve(&core, B), FIRST + 5, "sticky");
    }
    assert!(
        saved(&core).is_empty(),
        "nothing changed, nothing is written"
    );
    // a new machine takes the lowest number no entry references
    let core = dt2_core();
    stored(&core, map);
    settle(&core, &selection(&core, &[&[B, A]]));
    core.boot_index.set(0);
    unsafe {
        assert_eq!((resolve(&core, B), resolve(&core, A)), (FIRST + 5, FIRST));
    }
    assert_eq!(saved(&core).len(), 1);
}

#[test]
fn the_map_of_the_selection_record_is_read_once_and_moved_to_the_settings_key() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dt2_core();
    let mut old = [0u32; MAP_N];
    old[0] = A | (FIRST + 2);
    set_legacy(&core, old);
    core.boot_index.set(0);
    settle(&core, &selection(&core, &[&[A]]));
    unsafe {
        assert_eq!(resolve(&core, A), FIRST + 2, "the number of the old map");
    }
    assert_eq!(
        saved(&core),
        [old],
        "saved to the new key although the boot changed nothing"
    );
    // nothing to migrate: nothing is saved for a boot without machines
    let core = dt2_core();
    settle(&core, &selection(&core, &[&[]]));
    assert!(saved(&core).is_empty());
}

#[test]
fn a_key_is_resolved_for_the_module_being_loaded_only_and_a_pinned_number_is_itself() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dt2_core();
    // the keys of the boot are unique (the planner refuses a repeat): each module resolves its own
    settle(&core, &selection(&core, &[&[A], &[B]]));
    unsafe {
        core.boot_index.set(0);
        assert_eq!(resolve(&core, A), FIRST);
        assert_eq!(resolve(&core, B), 0, "a key of another module");
        core.boot_index.set(1);
        assert_eq!(resolve(&core, B), FIRST + 1);
        assert_eq!(resolve(&core, A), 0, "a key of another module");
        core.boot_index.set(u32::MAX);
        assert_eq!(resolve(&core, A), 0, "outside a module load");
        // a key without a hash is a pinned number: in the run-time range it is itself, outside it none
        assert_eq!(resolve(&core, FIRST + 3), FIRST + 3);
        assert_eq!(resolve(&core, 0x20), 0);
    }
    assert_eq!(core.boot_index(), u32::MAX);
}

#[test]
fn a_refused_write_is_tried_again_at_the_next_boot_with_the_same_numbers_and_the_old_bytes_stay() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dt2_core();
    let mut old = [0u32; MAP_N];
    old[0] = A | (FIRST + 2);
    set_legacy(&core, old);
    core.put_result.set(E_BUSY);
    core.boot_index.set(0);
    settle(&core, &selection(&core, &[&[A, B]]));
    let first = unsafe { (resolve(&core, A), resolve(&core, B)) };
    assert_eq!(
        first,
        (FIRST + 2, FIRST),
        "the number of the old map, then the lowest free"
    );
    assert_eq!(saved(&core).len(), 1, "one write, refused");
    assert_ne!(
        core.legacy.get(),
        [0u8; 32],
        "the old bytes are not zeroed while the key is not stored"
    );
    assert!(core.settings.borrow().is_empty());
    // the next boot: the same numbers, and the write is tried again
    core.put_result.set(OK);
    settle(&core, &selection(&core, &[&[A, B]]));
    assert_eq!(unsafe { (resolve(&core, A), resolve(&core, B)) }, first);
    assert_eq!(saved(&core).len(), 2);
    assert_eq!(
        core.legacy.get(),
        [0u8; 32],
        "zeroed after the key holds the map"
    );
}

#[test]
fn after_the_migration_an_erased_key_does_not_bring_back_the_old_map() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = dt2_core();
    let mut old = [0u32; MAP_N];
    old[0] = A | (FIRST + 2);
    set_legacy(&core, old);
    core.boot_index.set(0);
    settle(&core, &selection(&core, &[&[A]]));
    assert_eq!(unsafe { resolve(&core, A) }, FIRST + 2);
    assert_eq!(core.legacy.get(), [0u8; 32]);
    // the key is erased: the machine takes the lowest number, not the old one
    core.settings.borrow_mut().clear();
    settle(&core, &selection(&core, &[&[A]]));
    assert_eq!(unsafe { resolve(&core, A) }, FIRST);
    // a refused zeroing is tried again while the key holds the map
    let again = FakeCore::new();
    set_legacy(&again, old);
    again.legacy_clear_result.set(E_BUSY);
    settle(&again, &selection(&again, &[&[A]]));
    assert_ne!(again.legacy.get(), [0u8; 32]);
    again.legacy_clear_result.set(OK);
    settle(&again, &selection(&again, &[&[A]]));
    assert_eq!(again.legacy.get(), [0u8; 32]);
}

#[test]
fn a_second_settle_over_the_live_set_keeps_every_number_and_gives_a_key_a_reload_adds_a_free_one() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    const C: u32 = 0x00cc_00;
    let core = dt2_core();
    settle(&core, &selection(&core, &[&[A, B]]));
    core.boot_index.set(0);
    let first = unsafe { (resolve(&core, A), resolve(&core, B)) };
    assert_eq!(first, (FIRST, FIRST + 1));
    assert_eq!(saved(&core).len(), 1);
    // the same live set again (idempotent): the numbers stay, nothing more is written
    settle(&core, &selection(&core, &[&[A, B]]));
    assert_eq!(unsafe { (resolve(&core, A), resolve(&core, B)) }, first);
    assert_eq!(saved(&core).len(), 1, "nothing changed, nothing is saved");
    // a reload of module 0 whose new file adds key C: A and B keep their numbers, C takes the next free one
    settle(&core, &selection(&core, &[&[A, B, C]]));
    assert_eq!(
        unsafe { (resolve(&core, A), resolve(&core, B), resolve(&core, C)) },
        (FIRST, FIRST + 1, FIRST + 2)
    );
    assert_eq!(saved(&core).len(), 2);
}

#[test]
fn live_numbering_preserves_unsaved_types_until_restore_or_commit() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    const C: u32 = 0x00cc_00;
    let core = dt2_core();
    let mut old = [0u32; MAP_N];
    old[0] = A | (FIRST + 2);
    set_legacy(&core, old);
    core.put_result.set(E_BUSY);
    settle(&core, &selection(&core, &[&[A], &[B]]));
    let calls = core.calls().len();

    core.put_result.set(OK);
    let candidate = selection(&core, &[&[C], &[B]]);
    settle(&core, &SettleCtx::preview(candidate.count));
    core.boot_index.set(0);
    assert_eq!(unsafe { resolve(&core, C) }, FIRST + 1);
    core.boot_index.set(1);
    assert_eq!(unsafe { resolve(&core, B) }, FIRST);
    assert_eq!(core.calls().len(), calls);
    assert_ne!(core.legacy.get(), [0; 32]);

    settle(&core, &SettleCtx::restore(2));
    core.boot_index.set(0);
    assert_eq!(
        unsafe { (resolve(&core, A), resolve(&core, C)) },
        (FIRST + 2, 0)
    );
    core.boot_index.set(1);
    assert_eq!(unsafe { resolve(&core, B) }, FIRST);
    assert_eq!(core.calls().len(), calls);

    settle(&core, &SettleCtx::preview(candidate.count));
    settle(&core, &SettleCtx::commit(candidate.count));
    core.boot_index.set(0);
    assert_eq!(unsafe { resolve(&core, C) }, FIRST + 1);
    assert_eq!(core.legacy.get(), [0; 32]);
    assert_eq!(
        saved(&core)[1][..3],
        [A | (FIRST + 2), B | FIRST, C | (FIRST + 1)]
    );
}
