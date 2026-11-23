use super::*;
use crate::table::PoolTable;
use dkrt_test_support::FakeCore;
use std::boxed::Box;

fn table() -> &'static PoolCell {
    Box::leak(Box::new(PoolCell::new(PoolTable::new())))
}

#[test]
fn a_release_stops_the_peak_window_of_the_freed_client_only() {
    let core = FakeCore::new();
    let mut s = Box::new(Samples::new(Pool::none(), table()));
    assert!(s.tracks.reserve(4, 1, 0x1000, 0x2000));
    assert!(s.tracks.reserve(5, 1, 0x3000, 0x4000));
    s.peaks.set_pending(0);
    s.peaks.set_pending(1);
    DspPart::<FakeCore>::release(&mut *s, &core, 4);
    assert!(
        s.peaks.abandoned(0),
        "the window of the freed client is stopped"
    );
    assert!(!s.peaks.abandoned(1), "the other module keeps its window");
}

#[test]
fn a_release_stops_the_window_of_every_client_slot_of_the_module() {
    let core = FakeCore::new();
    let mut s = Box::new(Samples::new(Pool::none(), table()));
    assert!(s.tracks.reserve(4, 1, 0x1000, 0x2000));
    assert!(s.tracks.reserve(4, 2, 0x3000, 0x4000));
    s.peaks.set_pending(0);
    s.peaks.set_pending(1);
    DspPart::<FakeCore>::release(&mut *s, &core, 4);
    assert!(s.peaks.abandoned(0) && s.peaks.abandoned(1));
}
