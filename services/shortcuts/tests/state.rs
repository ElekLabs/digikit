//! Host tests of the owner objects: `KeyState`, `BindingTable` and `Shortcuts` (no module; the owner's `Provider` and `Ticker` parts over a `FakeCore`). The callbacks append to a
//! log that the binding's `arg` points to.
use core::ffi::c_void;
use dkrt_abi::platform::{DeviceProfile, KEY_SLOTS};
use dkrt_abi::profiles::ALL;
use dkrt_abi::provider::{ConsumerId, Provider, Ticker};
use dkrt_abi::service::tick_phase;
use dkrt_svc_shortcuts::bindings::{Added, Binding, BindingTable, MAX_BINDINGS};
use dkrt_svc_shortcuts::fake_keys::FakeKeys;
use dkrt_svc_shortcuts::keys::KeyState;
use dkrt_svc_shortcuts::shortcuts::{self, key_flags, Verdict};
use dkrt_test_support::FakeCore;

/// The owner over the fake subscription: nothing here reaches a platform.
type Shortcuts = shortcuts::Shortcuts<FakeKeys>;

// logical key numbers of these tests, and the panel codes they have on the test device
const TRK: u8 = 1;
const YES: u8 = 2;
const TRIG1: u8 = 3;
const OTHER: u8 = 4;
const C_TRK: u32 = 16;
const C_YES: u32 = 10;
const C_TRIG1: u32 = 25;
const C_OTHER: u32 = 11;
const RELEASE: u32 = key_flags::RELEASE;
const REPEAT: u32 = key_flags::REPEAT;

type Log = Vec<&'static str>;

unsafe extern "C" fn on(arg: *mut c_void) {
    (*(arg as *mut Log)).push("on");
}
unsafe extern "C" fn off(arg: *mut c_void) {
    (*(arg as *mut Log)).push("off");
}
unsafe extern "C" fn on_b(arg: *mut c_void) {
    (*(arg as *mut Log)).push("on-b");
}

fn device() -> DeviceProfile {
    let mut p = ALL[0];
    p.key_codes = [0; KEY_SLOTS];
    p.key_codes[TRK as usize] = C_TRK as u8;
    p.key_codes[YES as usize] = C_YES as u8;
    p.key_codes[TRIG1 as usize] = C_TRIG1 as u8;
    p.key_codes[OTHER as usize] = C_OTHER as u8;
    p
}

fn binding(owner: u8, keys: [u8; 2], log: &mut Log) -> Binding {
    let mut b = Binding::free();
    b.owner = owner;
    b.keys = keys;
    b.on = Some(on);
    b.off = Some(off);
    b.arg = log as *mut Log as *mut c_void;
    b
}

/// The UI pass of a phase: `Ticker::tick`.
fn pass(s: &mut Shortcuts, phase: u32) {
    Ticker::tick(s, &FakeCore::new(), phase);
}

/// The module of `owner` unloads: `Provider::detach`.
fn detach(s: &mut Shortcuts, owner: u8) {
    Provider::detach(s, &FakeCore::new(), ConsumerId::new(owner));
}

/// One key event and the UI pass after it; the verdict. (`event` below does not run the pass.)
fn key(s: &mut Shortcuts, p: &DeviceProfile, code: u32, flags: u32) -> Verdict {
    let v = s.on_key(p, code, flags);
    pass(s, tick_phase::EARLY);
    v
}

/// One key event only: the module's calls wait for the UI pass.
fn event(s: &mut Shortcuts, p: &DeviceProfile, code: u32, flags: u32) -> Verdict {
    s.on_key(p, code, flags)
}

#[test]
fn key_state_holds_a_key_from_its_press_to_its_release() {
    let mut k = KeyState::new();
    assert!(!k.is_held(5));
    k.press(5);
    k.press(5);
    assert!(k.is_held(5) && !k.is_held(6));
    k.release(5);
    assert!(!k.is_held(5));
    // DK_KEY_NONE and a number past the table are never held
    k.press(0);
    k.press(KEY_SLOTS as u32);
    assert!(!k.is_held(0) && !k.is_held(KEY_SLOTS as u32));
}

#[test]
fn the_first_owner_of_a_key_pair_keeps_its_binding_and_a_later_one_takes_over_when_it_leaves() {
    let mut log = Log::new();
    let mut t = BindingTable::new();
    assert_eq!(t.add(binding(1, [TRK, YES], &mut log)), Added::Enabled);
    let mut second = binding(2, [TRK, YES], &mut log);
    second.on = Some(on_b);
    assert_eq!(t.add(second), Added::Disabled { owner: 1 });
    // another pair is no clash
    assert_eq!(t.add(binding(2, [TRK, OTHER], &mut log)), Added::Enabled);
    assert_eq!(t.len(), 3);
    let held = |k: u32| k == u32::from(TRK);
    let i = t.completed(u32::from(YES), held).unwrap();
    assert_eq!(t.get(i).unwrap().owner, 1);
    t.remove(1);
    let i = t.completed(u32::from(YES), held).unwrap();
    assert_eq!(
        t.get(i).unwrap().owner,
        2,
        "the disabled binding is enabled when its keeper is gone"
    );
    assert!(t
        .get(i)
        .unwrap()
        .on
        .is_some_and(|f| f as *const () as usize == on_b as *const () as usize));
    t.remove(2);
    assert_eq!(t.len(), 0);
    assert!(t.completed(u32::from(YES), held).is_none());
}

#[test]
fn a_binding_needs_its_first_key_held_and_a_full_table_refuses() {
    let mut log = Log::new();
    let mut t = BindingTable::new();
    t.add(binding(1, [TRK, YES], &mut log));
    assert!(t.completed(u32::from(YES), |_| false).is_none());
    assert!(t
        .completed(u32::from(OTHER), |k| k == u32::from(TRK))
        .is_none());
    assert_eq!(t.count_of(1), 1);
    for i in 1..MAX_BINDINGS {
        assert_eq!(
            t.add(binding(2, [TRK, (10 + i) as u8], &mut log)),
            Added::Enabled
        );
    }
    assert_eq!(t.add(binding(3, [TRK, 60], &mut log)), Added::Full);
}

fn with_shortcut(p: &DeviceProfile) -> (Shortcuts, Box<Log>) {
    let _ = p;
    let mut log = Box::new(Log::new());
    let mut s = Shortcuts::new();
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    (s, log)
}

#[test]
fn holding_the_first_key_and_pressing_the_second_calls_on_then_off_on_release() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    // the first key is never swallowed
    assert_eq!(key(&mut s, &p, C_TRK, 0), Verdict::Pass);
    assert!(log.is_empty());
    // the second key completes the binding and is swallowed
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(*log, ["on"]);
    assert!(s.active().is_some());
    // its repeats and its release are swallowed, and the release calls `off`
    assert_eq!(key(&mut s, &p, C_YES, REPEAT), Verdict::Swallow);
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Swallow);
    assert_eq!(*log, ["on", "off"]);
    assert!(s.active().is_none());
    // releasing the first key afterwards passes and calls nothing
    assert_eq!(key(&mut s, &p, C_TRK, RELEASE), Verdict::Pass);
    assert_eq!(*log, ["on", "off"]);
}

#[test]
fn releasing_the_first_key_first_calls_off_once_and_still_swallows_the_second_keys_release() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    key(&mut s, &p, C_TRK, 0);
    key(&mut s, &p, C_YES, 0);
    assert_eq!(key(&mut s, &p, C_TRK, RELEASE), Verdict::Pass);
    assert_eq!(*log, ["on", "off"]);
    // the stock firmware never saw the press of the second key, so it does not see its release
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Swallow);
    assert_eq!(*log, ["on", "off"]);
    // a later plain press of the second key is not a swallowed one
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Pass);
}

#[test]
fn other_keys_pass_while_a_binding_is_active_and_the_first_keys_repeat_passes() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    key(&mut s, &p, C_TRK, 0);
    key(&mut s, &p, C_YES, 0);
    // a trig key under the first key selects a track as stock, and it does not end the binding
    assert_eq!(key(&mut s, &p, C_TRIG1, 0), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_TRIG1, RELEASE), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_TRK, REPEAT), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_OTHER, 0), Verdict::Pass);
    assert_eq!(*log, ["on"]);
    assert!(s.active().is_some());
}

#[test]
fn the_second_key_alone_or_before_the_first_key_does_nothing() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_TRK, 0), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Pass);
    assert!(log.is_empty() && s.active().is_none());
}

#[test]
fn a_panel_code_without_a_logical_key_passes() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    assert_eq!(key(&mut s, &p, 99, 0), Verdict::Pass);
    assert_eq!(key(&mut s, &p, 0, 0), Verdict::Pass);
    assert!(log.is_empty());
}

#[test]
fn a_key_the_device_lacks_leaves_the_shortcut_inactive() {
    let mut p = device();
    p.key_codes[YES as usize] = 0;
    let (mut s, log) = with_shortcut(&p);
    key(&mut s, &p, C_TRK, 0);
    // the code that YES has on the other device is no key of this one
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Pass);
    assert!(log.is_empty() && s.active().is_none());
}

#[test]
fn detaching_the_owner_of_the_active_binding_ends_it_without_calling_its_code() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    key(&mut s, &p, C_TRK, 0);
    key(&mut s, &p, C_YES, 0);
    detach(&mut s, 1);
    assert!(s.active().is_none() && s.table().len() == 0);
    assert_eq!(*log, ["on"], "the module's code is gone: no `off`");
    // the key state stays (the keys are still down): the second key's release is swallowed once, as the stock firmware never saw the press
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Swallow);
    assert_eq!(
        key(&mut s, &p, C_YES, RELEASE),
        Verdict::Pass,
        "swallowed once"
    );
    // the first key is still held: a binding that a module adds now completes with it
    let mut log2 = Box::new(Log::new());
    s.table_mut().add(binding(2, [TRK, YES], &mut log2));
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(*log2, ["on"]);
    // nothing is bound once that module is gone too
    detach(&mut s, 2);
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Swallow);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Pass);
}

#[test]
fn a_binding_that_is_not_active_is_not_ended_by_another_owner_leaving() {
    let p = device();
    let mut log = Box::new(Log::new());
    let mut s = Shortcuts::new();
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    s.table_mut().add(binding(2, [TRK, OTHER], &mut log));
    key(&mut s, &p, C_TRK, 0);
    key(&mut s, &p, C_YES, 0);
    detach(&mut s, 2);
    assert!(s.active().is_some_and(|b| b.owner == 1));
    key(&mut s, &p, C_YES, RELEASE);
    assert_eq!(*log, ["on", "off"]);
}

#[test]
fn only_one_binding_is_active_and_the_first_owner_of_a_pair_is_the_one_called() {
    let p = device();
    let mut log = Box::new(Log::new());
    let mut s = Shortcuts::new();
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    s.table_mut().add(binding(2, [TRK, YES], &mut log));
    s.table_mut().add(binding(2, [TRK, OTHER], &mut log));
    key(&mut s, &p, C_TRK, 0);
    key(&mut s, &p, C_YES, 0);
    assert_eq!(
        *log,
        ["on"],
        "the second owner's binding of the same pair is disabled"
    );
    // a second combination while the first is active passes
    assert_eq!(key(&mut s, &p, C_OTHER, 0), Verdict::Pass);
    assert_eq!(*log, ["on"]);
}

#[test]
fn start_makes_an_owner_with_nothing_bound_held_or_queued_and_a_closed_builder() {
    let core = FakeCore::new();
    let mut s = dkrt_test_support::start_boxed::<Shortcuts>(&core).unwrap();
    assert!(s.active().is_none() && s.table().len() == 0 && s.pending() == 0);
    let p = device();
    let mut log = Box::new(Log::new());
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    assert_eq!(
        key(&mut s, &p, C_YES, 0),
        Verdict::Pass,
        "the second key alone passes: no key is held"
    );
    assert!(s.active().is_none());
    key(&mut s, &p, C_YES, RELEASE);
    key(&mut s, &p, C_TRK, 0);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(*log, ["on"]);
}

#[test]
fn the_hook_runs_no_module_code_the_ui_pass_makes_the_calls_in_order() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    assert_eq!(event(&mut s, &p, C_TRK, 0), Verdict::Pass);
    assert_eq!(event(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert!(log.is_empty(), "on_key called no module code");
    assert_eq!(s.pending(), 1);
    // a fast tap: the release comes before the UI pass; on is still called before off
    assert_eq!(event(&mut s, &p, C_YES, RELEASE), Verdict::Swallow);
    assert!(log.is_empty());
    assert_eq!(s.pending(), 2);
    pass(&mut s, tick_phase::EARLY);
    assert_eq!(*log, ["on", "off"]);
    assert_eq!(s.pending(), 0);
    pass(&mut s, tick_phase::EARLY);
    assert_eq!(*log, ["on", "off"], "each call is made once");
}

#[test]
fn a_detach_drops_the_calls_of_that_owner_that_wait() {
    let p = device();
    let mut log = Box::new(Log::new());
    let mut log2 = Box::new(Log::new());
    let mut s = Shortcuts::new();
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    s.table_mut().add(binding(2, [TRK, OTHER], &mut log2));
    event(&mut s, &p, C_TRK, 0);
    event(&mut s, &p, C_YES, 0);
    event(&mut s, &p, C_YES, RELEASE);
    event(&mut s, &p, C_OTHER, 0);
    assert_eq!(s.pending(), 3);
    detach(&mut s, 1);
    assert_eq!(s.pending(), 1, "only the calls of owner 2 are left");
    pass(&mut s, tick_phase::EARLY);
    assert!(log.is_empty(), "owner 1's code is gone: nothing of it runs");
    assert_eq!(*log2, ["on"]);
}

#[test]
fn a_press_of_a_swallowed_key_is_swallowed_again_and_calls_on_once() {
    // the firmware may encode a repeat as a plain press
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    key(&mut s, &p, C_TRK, 0);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(*log, ["on"]);
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Swallow);
    assert_eq!(*log, ["on", "off"]);
}

/// The firmware's flags word is read with the device's bits: a clear press bit is a release whatever the other bits say; the repeat bit on a press is a repeat.
#[test]
fn the_firmware_flags_become_release_repeat_or_press() {
    use dkrt_svc_shortcuts::shortcuts::classify_flags;
    for p in ALL {
        let b = p.key_flags;
        let bit = |n: u8| 1u32 << n;
        assert_eq!(classify_flags(&p, bit(b.press)), 0, "{}: press", p.name);
        assert_eq!(
            classify_flags(&p, bit(b.press) | bit(b.func)),
            0,
            "{}: press with FUNC",
            p.name
        );
        assert_eq!(
            classify_flags(&p, bit(b.press) | bit(b.repeat)),
            REPEAT,
            "{}: repeat",
            p.name
        );
        assert_eq!(
            classify_flags(&p, bit(b.release)),
            RELEASE,
            "{}: release",
            p.name
        );
        assert_eq!(classify_flags(&p, 0), RELEASE, "{}: no bit", p.name);
        assert_eq!(
            classify_flags(&p, bit(b.repeat)),
            RELEASE,
            "{}: a clear press bit is a release",
            p.name
        );
    }
}

#[test]
fn the_tick_makes_the_queued_calls_in_order_in_the_early_phase_only() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    event(&mut s, &p, C_TRK, 0);
    event(&mut s, &p, C_YES, 0);
    event(&mut s, &p, C_YES, RELEASE);
    assert_eq!(s.pending(), 2);
    for phase in [tick_phase::LATE, tick_phase::LAST] {
        pass(&mut s, phase);
        assert!(log.is_empty(), "phase {phase}: no call is made");
        assert_eq!(s.pending(), 2);
    }
    pass(&mut s, tick_phase::EARLY);
    assert_eq!(*log, ["on", "off"], "on before off, as the events came");
    assert_eq!(s.pending(), 0);
}

#[test]
fn a_detach_of_an_owner_that_never_attached_changes_nothing() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    event(&mut s, &p, C_TRK, 0);
    event(&mut s, &p, C_YES, 0);
    detach(&mut s, 7);
    assert_eq!((s.table().len(), s.pending()), (1, 1));
    assert!(s.active().is_some());
    pass(&mut s, tick_phase::EARLY);
    assert_eq!(*log, ["on"]);
}

#[test]
fn start_in_gives_the_initial_state_over_whatever_the_slot_held() {
    use core::mem::MaybeUninit;
    let core = FakeCore::new();
    let mut slot = MaybeUninit::<Shortcuts>::uninit();
    // a slot that holds the bytes of an earlier life
    unsafe { slot.as_mut_ptr().write_bytes(0xA5, 1) };
    <Shortcuts as Provider<FakeCore>>::start_in(&core, &mut slot).unwrap();
    // SAFETY: `start_in` initialised the slot.
    let s = unsafe { slot.assume_init_mut() };
    assert_eq!((s.pending(), s.table().len()), (0, 0));
    assert!(s.active().is_none());
    // the builder gate is closed: a call outside `init` is refused
    assert_eq!(
        unsafe {
            s.builder()
                .add(&core, core::ptr::null_mut(), core::ptr::null())
        },
        dkrt_sdk::E_CONTEXT
    );
    // and it works like a state made by `start`
    let mut log = Log::new();
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    assert_eq!(s.table().len(), 1);
}

/// A service reload while a swallowed key is held: the new owner never saw the press, so the release is the firmware's (`Pass`) and queues nothing. The module that bound the key is
/// down meanwhile (its `off` was dropped by `detach`, as for an unload).
#[test]
fn a_service_started_again_passes_the_release_of_a_key_it_never_saw_pressed_and_calls_nothing() {
    let p = device();
    let core = FakeCore::new();
    let mut s = dkrt_test_support::start_boxed::<Shortcuts>(&core).unwrap();
    let mut log = Box::new(Log::new());
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    key(&mut s, &p, C_TRK, 0);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(*log, ["on"]);
    // the closure goes down: the module's bindings and the active one end without module code
    detach(&mut s, 1);
    assert_eq!(*log, ["on"], "detach calls no module code");
    drop(s);
    let mut s = dkrt_test_support::start_boxed::<Shortcuts>(&core).unwrap();
    assert!(s.active().is_none() && s.table().len() == 0 && s.pending() == 0);
    assert_eq!(key(&mut s, &p, C_YES, RELEASE), Verdict::Pass);
    assert_eq!(key(&mut s, &p, C_TRK, RELEASE), Verdict::Pass);
    assert_eq!((s.pending(), *log), (0, vec!["on"]));
}
