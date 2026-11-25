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
use file::fw::{
    click_tick, dk_param_get, dk_param_set, dks_page_key, dks_page_turn, page_key, param_get,
    param_set,
};
pub use file::{builder, fw, native, DK_SVC_OWNER};
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
    dkrt_svc_machines::records::publish(&dkrt_svc_rt::ImportCore);
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

/// A page whose every firmware read and write enters the owner as a hook of the firmware does (the page's code hook, the hooks a setter fires): the call is answered when the
/// owner is idle and `refused` counts the calls that found it busy.
#[derive(Clone)]
struct ReentrantFw {
    refused: std::rc::Rc<std::cell::Cell<u32>>,
    stored: std::rc::Rc<std::cell::Cell<Option<(usize, u32, i32, u32)>>>,
}

impl ReentrantFw {
    fn new() -> ReentrantFw {
        ReentrantFw {
            refused: Default::default(),
            stored: Default::default(),
        }
    }
    /// The owner is entered, as a hook of the firmware does it: `Some` when it was idle.
    fn enter(&self) -> Option<()> {
        let r = DK_SVC_OWNER.with(|_| ());
        if r.is_none() {
            self.refused.set(self.refused.get() + 1);
        }
        r
    }
}

impl dkrt_abi::fw::Fw for ReentrantFw {
    fn mode_key_held(&self) -> bool {
        false
    }
    fn page_value(&self, _: usize, _: u32) -> i32 {
        self.enter().map_or(-7, |()| 0x1200)
    }
    fn object(&self, _: usize, _: u32, _: u32) -> usize {
        0x77
    }
    fn object_value(&self, _: usize, _: u32) -> i32 {
        0
    }
    fn store(&self, obj: usize, code: u32, raw: i32, track: u32) {
        if self.enter().is_some() {
            self.stored.set(Some((obj, code, raw, track)));
        }
    }
    fn view_track(&self, _: usize) -> u32 {
        self.enter().map_or(99, |()| 5)
    }
    fn param_code(&self, _: usize, enc: u32) -> i32 {
        self.enter().map_or(-1, |()| enc as i32)
    }
}

/// The key hook asks the page for the encoder code and the track outside the owner: a page that enters the owner is answered, so the click of a native page's bound knob
/// reaches its action.
#[test]
fn a_click_maps_to_the_module_code_when_the_page_enters_the_owner() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _o = Started::new();
    // the stub image has no registry: the first parameter code is u32::MAX, so code 1 is action slot 2
    DK_SVC_OWNER.with(|m| m.clicks().set(2, 0x4000)).unwrap();
    let view = [0u32; 64];
    let v = view.as_ptr() as *const u8;
    let ev = |flags: u32| [0u32, 0, 0, 0x29 + 1, flags, 0];
    let (press, release) = (ev(0x01), ev(0x10));
    let fw = ReentrantFw::new();
    // SAFETY: the view and the events are zeroed words of the size the hook reads
    unsafe {
        page_key(|_| fw.clone(), v, press.as_ptr() as *const u8);
        page_key(|_| fw.clone(), v, release.as_ptr() as *const u8);
    }
    let p = DK_SVC_OWNER.with(|m| m.clicks().pending()).unwrap();
    assert_eq!(
        (p.callback, p.track, p.code),
        (0x4000, 5, 1),
        "the click maps to the module's encoder code and the page's track"
    );
    assert_eq!(fw.refused.get(), 0, "the page never found the owner busy");
}

fn open_flow() {
    DK_SVC_OWNER
        .with(|m| {
            m.open_flow_for_test(dkrt_svc_machines::paramapi::Ctx {
                view: 0x1000,
                track: 4,
                owner: 2,
            })
        })
        .unwrap();
}

/// `dk_param_get` reads the page outside the owner: a getter that enters the owner (as a hook does) is answered.
#[test]
fn param_get_reads_the_page_while_the_owner_is_idle() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _o = Started::new();
    open_flow();
    let fw = ReentrantFw::new();
    let v = unsafe { param_get(|_| fw.clone(), 4, 0) };
    assert_eq!(v, 0x1200, "the value the page gave");
    assert_eq!(fw.refused.get(), 0);
}

/// `dk_param_set` stores through the stock setter outside the owner: the hooks the setter fires reach the service.
#[test]
fn param_set_stores_while_the_owner_is_idle() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _o = Started::new();
    open_flow();
    let fw = ReentrantFw::new();
    let r = unsafe { param_set(|_| fw.clone(), 3, 0, 0x2a00) };
    assert_eq!(r, 0);
    assert_eq!(fw.refused.get(), 0);
    assert!(fw.stored.get().is_some(), "the setter ran");
    let none = unsafe { param_get(|_| fw.clone(), 3, 9) };
    assert_eq!(
        none,
        dkrt_svc_machines::paramapi::E_ARG,
        "a word the module lacks"
    );
}

/// A module's click callback (SHOT's open-picker) reads the page with `dk_param_get`: the click runs outside the owner, so the call is answered, not refused with `E_STATE`.
#[test]
fn a_click_callback_that_reads_a_parameter_gets_the_value() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _o = Started::new();
    // the stub image has no registry: code 1 is parameter row 2, action slot 2
    DK_SVC_OWNER
        .with(|m| {
            m.own_row_for_test(2, 3);
            m.clicks().set(2, 0x4000);
        })
        .unwrap();
    let view = [0u32; 64];
    let v = view.as_ptr() as *const u8;
    let ev = |flags: u32| [0u32, 0, 0, 0x29 + 1, flags, 0];
    let (press, release) = (ev(0x01), ev(0x10));
    let fw = ReentrantFw::new();
    // SAFETY: the view and the events are zeroed words of the size the hook reads
    unsafe {
        page_key(|_| fw.clone(), v, press.as_ptr() as *const u8);
        page_key(|_| fw.clone(), v, release.as_ptr() as *const u8);
        // the runtime's pass: the owner's tick part, then the file's tick
        DK_SVC_OWNER.with(|m| {
            dkrt_abi::provider::Ticker::tick(
                m,
                &dkrt_svc_rt::ImportCore,
                dkrt_abi::service::tick_phase::LAST,
            )
        });
    }
    let mut got = None;
    // SAFETY: the closure stands for the module's callback and reads through the fake page
    unsafe {
        click_tick(|_click| got = Some(param_get(|_| fw.clone(), 5, 0)));
    }
    assert_eq!(
        got,
        Some(0x1200),
        "the callback read the value the page gave"
    );
    assert_eq!(fw.refused.get(), 0);
}

/// A view whose reads enter the owner as the firmware's getters do when a hook runs inside them: `refused` counts the reads that found it busy.
struct ReentrantView {
    refused: std::cell::Cell<u32>,
    ty: i32,
}
impl ReentrantView {
    fn enter(&self) {
        if DK_SVC_OWNER.with(|_| ()).is_none() {
            self.refused.set(self.refused.get() + 1);
        }
    }
}
impl dkrt_svc_machines::native_page::View for ReentrantView {
    fn page_index(&self) -> u32 {
        self.enter();
        0
    }
    fn native_type(&self) -> i32 {
        self.enter();
        self.ty
    }
    fn track(&self) -> u32 {
        self.enter();
        0
    }
    fn value(&self, _: u32) -> i32 {
        self.enter();
        0
    }
    fn invalidate(&self) {}
}

/// The page lookup and the code hook read the view outside the owner: reads that enter it again (a hook inside a firmware getter) are answered, and the page is found.
#[test]
fn the_native_page_lookup_reads_the_view_while_the_owner_is_idle() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let _o = Started::new();
    DK_SVC_OWNER.with(|m| m.register_page_for_test(9)).unwrap();
    let view = ReentrantView {
        refused: Default::default(),
        ty: 0,
    };
    assert!(
        native::selected(&view).is_some(),
        "the registered page is found"
    );
    assert_eq!(
        native::code(&view, 3),
        9,
        "encoder 3 maps to the bound code"
    );
    assert_eq!(view.refused.get(), 0, "no read found the owner busy");
    let other = ReentrantView {
        refused: Default::default(),
        ty: 5,
    };
    assert!(native::selected(&other).is_none());
    assert_eq!(
        native::code(&other, 3),
        -1,
        "a type without pages resumes the stock mapper"
    );
    assert_eq!(other.refused.get(), 0);
}
