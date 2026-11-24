//! The service file's own code over the owner cell, on the host: the builder exports reach the owner through `DK_SVC_OWNER.with` and answer the neutral way when there is none (a
//! null handle, no effect), `dk_param_get` and `dk_param_set` answer "no flow" without an owner, the machine page's hooks pass without one, and the file's `init` step refuses an
//! image without the machines table and leaves the cell empty. The imports of the file (`dkc_*`) are stubs here, and the stub image has no tables.
use core::ptr::{null, null_mut};
use dkrt_sdk::E_NO_REGISTRY;
use dkrt_svc_machines::machines::Machines;
use dkrt_svc_machines::paramapi::E_STATE;
use std::sync::Mutex;

// the service file's source, as the ColdFire build has it: the builder calls reach the owner as `crate::DK_SVC_OWNER`
#[path = "../dks/src/lib.rs"]
pub mod file;
use file::builder::{
    dk_machine_add_page, dk_module_ctx_add_machine, dk_page_add_param, dk_page_bind,
    dk_page_bind_local, dk_param_glyph, dk_param_latched, dk_param_range,
};
use file::fw::{dk_param_get, dk_param_set, dks_page_key, dks_page_turn};
pub use file::{builder, fw, DK_SVC_OWNER};
use file::{dk_svc_start, dk_svc_stop};

#[path = "../../../runtime/core/test-support/stubs/core_imports.rs"]
mod stubs;

/// The cell is one per process: the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

/// The owner started in the cell, ended when the guard goes, also when the test fails.
struct Started;

impl Started {
    fn new() -> Started {
        DK_SVC_OWNER
            .start(|slot| {
                slot.write(Machines::new());
                Ok(())
            })
            .expect("start");
        Started
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        dk_svc_stop();
    }
}

const NAME: *const u8 = b"A\0".as_ptr();

#[test]
fn without_an_owner_a_builder_call_has_no_handle_and_the_flow_calls_say_no_flow() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
    // SAFETY: the calls read nothing when the cell is empty
    unsafe {
        assert!(dk_module_ctx_add_machine(null_mut(), 0, NAME, NAME).is_null());
        assert!(dk_machine_add_page(null_mut(), NAME, NAME).is_null());
        assert!(dk_page_add_param(null_mut(), NAME, NAME, 1).is_null());
        dk_param_range(null_mut(), 0, 1);
        dk_param_glyph(null_mut(), 1);
        dk_param_latched(null_mut());
        dk_page_bind(null_mut(), null_mut(), 0);
        dk_page_bind_local(null_mut(), 0, 0);
        assert_eq!(dk_param_get(0, 0), E_STATE);
        assert_eq!(dk_param_set(0, 0, 1), E_STATE);
        // the hooks pass: no click is looked for, and no turn is consumed
        dks_page_key(null(), null());
        assert_eq!(dks_page_turn(null(), null()), 0);
    }
}

#[test]
fn the_file_refuses_an_image_without_the_machines_table_and_keeps_no_owner() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(dk_svc_start(), Err(E_NO_REGISTRY));
    assert!(DK_SVC_OWNER.with(|_| ()).is_none(), "the cell stays empty");
}

#[test]
fn the_exports_reach_the_started_owner() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let started = Started::new();
    // the owner judges the calls: no `begin`, so its gate is closed and the placeholder comes back (not null)
    let ph = DK_SVC_OWNER.with(|m| m.builder().placeholder()).unwrap();
    // SAFETY: the calls compare the pointers and read no memory while the gate is closed
    unsafe {
        assert_eq!(dk_module_ctx_add_machine(null_mut(), 0, NAME, NAME), ph);
        assert_eq!(dk_machine_add_page(ph, NAME, NAME), ph);
        assert_eq!(dk_page_add_param(ph, NAME, NAME, 1), ph);
        // no click flow is open: the owner says so, as the cell did
        assert_eq!(dk_param_get(0, 0), E_STATE);
        // the turn reaches the native handler (the stub image binds none, so nothing is consumed)
        assert_eq!(dks_page_turn(null(), null()), 0);
    }
    drop(started);
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
}

#[test]
fn the_row_reaches_the_object_through_the_owner_guard() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    // the stub image has no machines table, so `dk_svc_start` refuses: the owner is started directly
    let _o = Started::new();
    dkrt_test_support::contract::row_is_guarded(
        file::dk_svc_row(),
        &|f| {
            DK_SVC_OWNER.with(|_| f());
        },
        &|| DK_SVC_OWNER.refused(),
    );
}

/// The MODULES reply asks `Describe` on the command handler's task, which may run while the UI task is inside a call of the owner (a tick, a hook, a window): the words are
/// answered at once, from the cells outside the owner, and the owner's guard is never asked.
#[test]
fn describe_answers_while_the_owner_is_busy_in_another_call() {
    use dkrt_abi::provider::ProviderHandle;
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _o = Started::new();
    // SAFETY: the row is a static of the file; the instance is the one `Started` made
    let handle = unsafe {
        ProviderHandle::resolve(
            file::dk_svc_row(),
            0..usize::MAX,
            DK_SVC_OWNER.instance() as *mut core::ffi::c_void,
        )
    }
    .unwrap();
    let core =
        &dkrt_svc_rt::ImportCore as *const dkrt_svc_rt::ImportCore as *const core::ffi::c_void;
    dkrt_svc_machines::rows::reset();
    dkrt_svc_machines::rows::vrows().mod_rows[1] = 0b101;
    let ask = || {
        let mut w = [0u32; 8];
        // SAFETY: `core` is the file's `ImportCore`, a zero-sized value
        let n = unsafe { handle.describe(core, 2, &mut w) };
        w[..n].to_vec()
    };
    let idle = ask();
    assert_eq!(
        idle,
        std::vec![0b101, 0, 0, 0, 0],
        "the voice rows of the module, nothing else"
    );
    // the owner is inside a call: a call into it is refused, the words are not
    let busy = DK_SVC_OWNER.with(|_| {
        assert!(
            DK_SVC_OWNER.with(|_| ()).is_none(),
            "the guard refuses a nested call"
        );
        ask()
    });
    assert_eq!(busy, Some(idle));
    dkrt_svc_machines::rows::reset();
}
