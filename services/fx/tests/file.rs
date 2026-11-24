//! The service file's own code over the owner cell, on the host: the builder exports reach the owner through `DK_SVC_OWNER.with` and answer the neutral way when there is none (a
//! null handle, no effect), the file's `init` step refuses an image without the effect pages and leaves the cell empty, and the owner's row is guarded.
//! The imports of the file (`dkc_*`) are stubs here, and the stub image has no tables.
use core::ptr::null_mut;
use dkrt_sdk::E_NO_REGISTRY;
use dkrt_svc_fx::fx::Fx;
use std::sync::Mutex;

// the service file's source, as the ColdFire build has it: the builder calls reach the owner as `crate::DK_SVC_OWNER`
#[path = "../dks/src/lib.rs"]
pub mod file;
use file::builder::{
    dk_fx_add_page, dk_fx_mix, dk_fx_routes, dk_module_ctx_add_fx, dk_page_add_fx_param,
};
pub use file::{builder, DK_SVC_OWNER};
use file::{dk_svc_start, dk_svc_stop};

// the core imports (`dkc_*`) of the service file, as host functions (this test links no runtime)
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
                slot.write(Fx::new());
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
fn without_an_owner_a_builder_call_has_no_handle() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
    // SAFETY: the calls read nothing when the cell is empty
    unsafe {
        assert!(dk_module_ctx_add_fx(null_mut(), 0, NAME, NAME).is_null());
        assert!(dk_fx_add_page(null_mut(), NAME, NAME).is_null());
        assert!(dk_page_add_fx_param(null_mut(), NAME, NAME, 1, 0.0, 1.0, 0.5).is_null());
        dk_fx_routes(null_mut(), 1, 1);
        dk_fx_mix(null_mut(), 5);
    }
}

#[test]
fn the_file_refuses_an_image_without_the_effect_pages_and_keeps_no_owner() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(dk_svc_start(), Err(E_NO_REGISTRY));
    assert!(DK_SVC_OWNER.with(|_| ()).is_none(), "the cell stays empty");
}

#[test]
fn the_exports_reach_the_started_owner() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let started = Started::new();
    // the owner judges the calls: no `begin`, so its gate is closed and the placeholder comes back (not null)
    let ph = DK_SVC_OWNER.with(|fx| fx.builder().placeholder()).unwrap();
    // SAFETY: the calls compare the pointers and read no memory while the gate is closed
    unsafe {
        assert_eq!(dk_module_ctx_add_fx(null_mut(), 0, NAME, NAME), ph);
        assert_eq!(dk_fx_add_page(ph, NAME, NAME), ph);
    }
    drop(started);
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
}

#[test]
fn the_row_reaches_the_object_through_the_owner_guard() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _started = Started::new();
    dkrt_test_support::contract::row_is_guarded(
        file::dk_svc_row(),
        &|f| {
            DK_SVC_OWNER.with(|_| f());
        },
        &|| DK_SVC_OWNER.refused(),
    );
}

#[test]
fn a_pick_that_arrives_while_the_owner_is_busy_is_queued_and_a_full_queue_says_busy() {
    use dkrt_svc_fx::routing::Event;
    use dkrt_svc_rt::Post;
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _started = Started::new();
    DK_SVC_OWNER
        .with(|_| {
            // the queue holds the picks of one call, none is lost
            for _ in 0..4 {
                assert_eq!(file::ui::submit(Event::Route(0)), Post::Queued);
            }
            assert_eq!(file::ui::submit(Event::Route(0)), Post::Full);
        })
        .expect("owner");
    assert_eq!(file::ui::refusal(Post::Full), Some("BUSY: TRY AGAIN"));
    assert_eq!(file::ui::refusal(Post::Queued), None);
    assert_eq!(file::ui::refusal(Post::Ran), None);
    // idle: the event runs at once
    assert_eq!(file::ui::submit(Event::List), Post::Ran);
}
