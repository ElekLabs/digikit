//! `ImageTable` (`dks/src/image_table.rs`): the binding of the image's `lfo_table_info`. The words are checked before anything is called, a missing table refuses the start,
//! and the two calls reach the image only for a wave of its range. A host build calls the stand-in table in memory.
use dkrt_sdk::{E_NO_REGISTRY, E_TABLE_FULL};
use dkrt_svc_lfo::wave_table::WaveTable;
use dkrt_test_support::FakeCore;

#[path = "../dks/src/image_table.rs"]
mod image_table;
use image_table::{host, ImageTable};

const GOOD: [u32; 7] = [0x4c46_4f54, 1, 7, 8, 32, 0x1000, 0x1010];

fn bound(core: &FakeCore) -> ImageTable {
    // SAFETY: the table is valid as zero bytes (its contract)
    let mut t: ImageTable = unsafe { core::mem::zeroed() };
    t.open(core).unwrap();
    t
}

#[test]
fn the_words_of_a_table_this_service_knows_bind_and_any_other_refuses() {
    assert!(ImageTable::parse(&GOOD).is_some());
    for (word, value) in [(0, 0), (1, 2), (3, 0), (4, 16), (5, 0), (6, 0)] {
        let mut w = GOOD;
        w[word] = value;
        assert!(ImageTable::parse(&w).is_none(), "word {word} = {value}");
    }
}

#[test]
fn an_image_without_the_table_refuses_and_one_with_it_holds_its_waves() {
    let mut core = FakeCore::new();
    // SAFETY: as above
    let mut t: ImageTable = unsafe { core::mem::zeroed() };
    assert_eq!(t.open(&core), Err(E_NO_REGISTRY));
    assert!(!t.holds(7), "unbound holds nothing");
    core.set_lfo_table(7, 4, 0x1000, 0x1010);
    let t = bound(&core);
    assert_eq!(
        (t.holds(6), t.holds(7), t.holds(10), t.holds(11)),
        (false, true, true, false)
    );
}

#[test]
fn the_calls_reach_the_image_for_a_wave_of_its_range_only() {
    let _image = host::install();
    let mut core = FakeCore::new();
    core.set_lfo_table(7, 4, 0x1000, 0x1010);
    let mut t = bound(&core);
    let s: [i16; 32] = core::array::from_fn(|i| 100 * i as i16);
    assert_eq!(t.set(8, &s), Ok(()));
    assert_eq!(host::playing(8), Some(s));
    assert_eq!(t.set(11, &s), Err(E_TABLE_FULL));
    assert_eq!(t.set(6, &s), Err(E_TABLE_FULL));
    assert_eq!(host::calls(), (1, 0));
    t.clear(8);
    t.clear(12);
    assert_eq!((host::playing(8), host::calls()), (None, (1, 1)));
}
