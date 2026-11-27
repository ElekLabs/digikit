//! The owner object `Lfo`: its initial state is all-zero bytes (`Provider::start_in` writes them where the file's cell lives), and it knows no number before `settle`.
use core::mem::MaybeUninit;
use dkrt_abi::provider::Provider;
use dkrt_svc_lfo::lfo::Lfo;
use dkrt_test_support::FakeCore;

#[path = "support/fake_wave_table.rs"]
mod fake_wave_table;
use fake_wave_table::FakeWaveTable;

type Owner = Lfo<FakeWaveTable>;

#[test]
fn start_in_over_a_dirty_slot_gives_the_initial_state() {
    let core = FakeCore::new();
    let mut slot = MaybeUninit::<Owner>::uninit();
    // SAFETY: filling the slot's bytes with a pattern; `Lfo` is valid for any bytes
    unsafe { slot.as_mut_ptr().write_bytes(0xa5, 1) };
    <Owner as Provider<FakeCore>>::start_in(&core, &mut slot).unwrap();
    // SAFETY: `start_in` wrote the state
    let lfo = unsafe { slot.assume_init() };
    assert_eq!(lfo.number_of(0, 0x00a1_0100), 0);
    assert_eq!(lfo.table().calls(), []);
}
