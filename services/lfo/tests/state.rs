//! The owner object `Lfo`: its initial state is all-zero bytes (`Provider::start_in` writes them where the file's cell lives), and it knows no number before `settle`.
use core::mem::MaybeUninit;
use dkrt_abi::provider::Provider;
use dkrt_svc_lfo::lfo::Lfo;
use dkrt_test_support::FakeCore;

#[test]
fn the_initial_state_is_all_zero_bytes() {
    let new = Lfo::new();
    // SAFETY: plain integers and arrays of them
    let bytes = unsafe {
        core::slice::from_raw_parts(&new as *const Lfo as *const u8, core::mem::size_of::<Lfo>())
    };
    assert!(bytes.iter().all(|b| *b == 0));
}

#[test]
fn start_in_over_a_dirty_slot_gives_the_initial_state() {
    let core = FakeCore::new();
    let mut slot = MaybeUninit::<Lfo>::uninit();
    // SAFETY: filling the slot's bytes with a pattern; `Lfo` is valid for any bytes
    unsafe { slot.as_mut_ptr().write_bytes(0xa5, 1) };
    <Lfo as Provider<FakeCore>>::start_in(&core, &mut slot).unwrap();
    // SAFETY: `start_in` wrote the state
    let lfo = unsafe { slot.assume_init() };
    assert_eq!(lfo.number_of(0, 0x00a1_0100), 0);
}

#[test]
fn start_and_start_in_make_the_same_bytes() {
    let core = FakeCore::new();
    let made = <Lfo as Provider<FakeCore>>::start(&core).unwrap();
    // SAFETY: plain integers and arrays of them
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &made as *const Lfo as *const u8,
            core::mem::size_of::<Lfo>(),
        )
    };
    assert!(bytes.iter().all(|b| *b == 0));
}
