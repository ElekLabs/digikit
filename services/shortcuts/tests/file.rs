//! The service file's own code over the owner cell, on the host: the builder export `dk_shortcut_add` and the key hook `dk_shortcuts_key` reach the owner through
//! `DK_SVC_OWNER.with`, and answer the neutral way when there is none. The imports of the file (`dkc_*`) are stubs here.
use core::sync::atomic::Ordering::SeqCst;
use dkrt_abi::profiles::ALL;
use dkrt_abi::provider::Records;
use dkrt_sdk::keys::{TRK, YES};
use dkrt_sdk::{E_CONTEXT, E_FORMAT};
use dkrt_svc_rt::ImportCore;
use dkrt_svc_shortcuts::bindings::Binding;
use dkrt_svc_shortcuts::shortcuts::Shortcuts;
use std::sync::Mutex;

// the service file's source, as the ColdFire build has it: the key hook and the builder call reach the owner as `crate::DK_SVC_OWNER`
#[path = "../dks/src/lib.rs"]
pub mod file;
use file::builder::dk_shortcut_add;
use file::hooks::dk_shortcuts_key;
pub use file::{builder, DK_SVC_OWNER};
use file::{dk_svc_start, dk_svc_stop};

mod stubs;
use stubs::{DEVICE, RUNNING};

/// The cell is one per process: the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

#[test]
fn without_an_owner_the_export_and_the_hook_answer_the_neutral_way() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
    // SAFETY: a null definition is refused before it is read; the hook runs no module code
    assert_eq!(
        unsafe { dk_shortcut_add(core::ptr::null_mut(), core::ptr::null()) },
        E_CONTEXT
    );
    assert_eq!(
        unsafe { dk_shortcuts_key(1, 0) },
        0,
        "the stock walker runs"
    );
}

#[test]
fn the_export_reaches_the_started_owner_and_the_hook_swallows_a_completed_binding() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let p = ALL
        .iter()
        .find(|p| p.key_code(TRK) != 0 && p.key_code(YES) != 0)
        .expect("a profile with TRK and YES");
    DEVICE.store(p.id, SeqCst);
    dk_svc_start().expect("start");
    // the gate is closed until the collection opens: the owner answers, not the fallback
    assert_eq!(
        unsafe { dk_shortcut_add(core::ptr::null_mut(), core::ptr::null()) },
        E_CONTEXT
    );
    DK_SVC_OWNER.with(|s| Records::<ImportCore>::begin(s, &ImportCore, 0));
    RUNNING.store(1, SeqCst);
    assert_eq!(
        unsafe { dk_shortcut_add(core::ptr::null_mut(), core::ptr::null()) },
        E_FORMAT,
        "the owner's builder judged the call"
    );
    // a binding in the owner's table: the hook answers from the owner's state
    DK_SVC_OWNER.with(|s: &mut Shortcuts| {
        let mut b = Binding::free();
        b.owner = 1;
        b.keys = [TRK as u8, YES as u8];
        b.on = None;
        s.table_mut().add(b)
    });
    let (trk, yes) = (p.key_code(TRK), p.key_code(YES));
    // the press flag of the device's key word is set for a press (`[input.key_flags]`)
    let press = 1u32 << p.key_flags.press;
    assert_eq!(
        unsafe { dk_shortcuts_key(trk, press) },
        0,
        "the first key passes"
    );
    assert_eq!(
        unsafe { dk_shortcuts_key(yes, press) },
        1,
        "the second key is swallowed"
    );
    dk_svc_stop();
    assert_eq!(
        unsafe { dk_shortcuts_key(yes, press) },
        0,
        "after fini the stock walker runs"
    );
}
