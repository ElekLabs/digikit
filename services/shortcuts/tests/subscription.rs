//! The service's subscription to the platform's key walk (`KeyEvents`) over `FakeKeys`: it subscribes when it starts and unsubscribes when it stops, a refusal fails the start, and the words of
//! the image's `input_keys_info` are read as `KeyTable` says.
use core::mem::MaybeUninit;
use dkrt_abi::provider::Provider;
use dkrt_sdk::{E_EXISTS, E_NO_REGISTRY, E_TABLE_FULL};
use dkrt_svc_shortcuts::fake_keys::FakeKeys;
use dkrt_svc_shortcuts::key_table::{KeyTable, WORDS};
use dkrt_svc_shortcuts::shortcuts::Shortcuts;
use dkrt_test_support::FakeCore;

/// A started owner over the fake, in its slot, or the refusal of the start.
fn start<const REFUSAL: i32>(
    slot: &mut MaybeUninit<Shortcuts<FakeKeys<REFUSAL>>>,
) -> Result<&mut Shortcuts<FakeKeys<REFUSAL>>, i32> {
    Provider::start_in(&FakeCore::new(), slot)?;
    // SAFETY: `start_in` made the instance
    Ok(unsafe { slot.assume_init_mut() })
}

#[test]
fn the_service_subscribes_when_it_starts_and_unsubscribes_when_it_stops() {
    let mut slot = MaybeUninit::uninit();
    let s = start::<0>(&mut slot).expect("the start");
    assert!(s.keys().subscribed());
    assert_eq!(
        (s.keys().subscribes(), s.keys().unsubscribes()),
        (1, 0),
        "one subscribe, made by the start"
    );
    Provider::stop(s, &FakeCore::new());
    assert!(!s.keys().subscribed());
    assert_eq!(s.keys().unsubscribes(), 1);
}

/// The refusal of the start over a fake that refuses every subscribe with `REFUSAL`.
fn refusal_of<const REFUSAL: i32>() -> Option<i32> {
    let mut slot = MaybeUninit::uninit();
    start::<REFUSAL>(&mut slot).err()
}

#[test]
fn a_refused_subscription_fails_the_start() {
    assert_eq!(refusal_of::<{ E_NO_REGISTRY }>(), Some(E_NO_REGISTRY));
    assert_eq!(refusal_of::<{ E_TABLE_FULL }>(), Some(E_TABLE_FULL));
    assert_eq!(refusal_of::<{ E_EXISTS }>(), Some(E_EXISTS));
}

#[test]
fn the_table_words_of_the_image_are_read_and_a_foreign_table_is_refused() {
    let words = [0x4b45_5953, 1, 4, 0x4011_0000, 0x4011_0040];
    let t = KeyTable::parse(&words).expect("the table");
    assert_eq!(
        (t.slots(), t.subscribe(), t.unsubscribe()),
        (4, 0x4011_0000, 0x4011_0040)
    );
    let bad = |i: usize, v: u32| {
        let mut w = words;
        w[i] = v;
        assert_eq!(KeyTable::parse(&w), None, "word {i} = {v:#x}");
    };
    bad(0, 0x4c46_4f54); // another table
    bad(1, 2); // another version
    bad(2, 0); // no slot
    bad(3, 0); // no subscribe entry
    bad(4, 0); // no unsubscribe entry
    assert_eq!(WORDS, words.len());
}
