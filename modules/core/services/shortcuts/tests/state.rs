//! Host tests of the owner objects: `KeyState`, `BindingTable` and `Shortcuts` (no core, no module). The callbacks append to a log that the binding's `arg` points to.
use core::ffi::c_void;
use dkrt_abi::platform::{DeviceProfile, KEY_SLOTS};
use dkrt_abi::profiles::ALL;
use dkrt_svc_shortcuts::bindings::{Added, Binding, BindingTable, MAX_BINDINGS};
use dkrt_svc_shortcuts::keys::KeyState;
use dkrt_svc_shortcuts::shortcuts::{key_flags, Shortcuts, Verdict};

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

/// One key event; the verdict.
fn key(s: &mut Shortcuts, p: &DeviceProfile, code: u32, flags: u32) -> Verdict {
    unsafe { s.on_key(p, code, flags) }
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
fn removing_the_owner_of_the_active_binding_ends_it_without_calling_its_code() {
    let p = device();
    let (mut s, log) = with_shortcut(&p);
    key(&mut s, &p, C_TRK, 0);
    key(&mut s, &p, C_YES, 0);
    s.remove(1);
    assert!(s.active().is_none() && s.table().len() == 0);
    assert_eq!(*log, ["on"], "the module's code is gone: no `off`");
    assert_eq!(
        key(&mut s, &p, C_YES, RELEASE),
        Verdict::Swallow,
        "the stock firmware never saw the press"
    );
    // nothing is bound now
    key(&mut s, &p, C_YES, 0);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Pass);
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
fn all_zero_bytes_are_the_initial_state_of_the_service_static() {
    // the service keeps `Shortcuts` in a zeroed static (`.bss`): nothing may be active in it
    let p = device();
    let mut log = Box::new(Log::new());
    let mut s: Shortcuts = unsafe { core::mem::zeroed() };
    assert!(s.active().is_none() && s.table().len() == 0);
    s.table_mut().add(binding(1, [TRK, YES], &mut log));
    assert_eq!(
        key(&mut s, &p, C_YES, 0),
        Verdict::Pass,
        "the second key alone passes"
    );
    assert!(s.active().is_none());
    key(&mut s, &p, C_YES, RELEASE);
    key(&mut s, &p, C_TRK, 0);
    assert_eq!(key(&mut s, &p, C_YES, 0), Verdict::Swallow);
    assert_eq!(*log, ["on"]);
}
