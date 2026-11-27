use super::*;

/// A range of 256 bytes taken at offset 0 (record 0) and one at 256 (record 1): the next fit is behind them; a hole left by record 0 is reused.
#[test]
fn first_fit_goes_past_the_ranges_and_reuses_a_hole() {
    let mut a = Area::new(0x1000, 1024);
    assert_eq!(a.first_fit(256, 64), Some(0));
    a.take(0, 0, 256);
    a.take(1, 256, 256);
    assert_eq!(a.first_fit(256, 64), Some(512));
    a.release(0);
    assert_eq!(a.first_fit(256, 64), Some(0));
    assert_eq!(a.first_fit(257, 64), Some(512));
}

#[test]
fn first_fit_keeps_the_alignment_and_refuses_what_does_not_fit() {
    let a = Area::new(0x1002, 64);
    assert_eq!(
        a.first_fit(16, 4),
        Some(2),
        "the byte address 0x1004 is the first aligned one"
    );
    assert_eq!(a.first_fit(65, 1), None);
    assert_eq!(Area::new(0, 0).first_fit(0, 1), Some(0));
}

#[test]
fn stats_count_the_free_bytes_and_the_largest_gap() {
    let mut a = Area::new(0, 1000);
    assert_eq!(a.stats(), (1000, 1000));
    a.take(0, 100, 200);
    a.take(1, 500, 100);
    // gaps: 0..100, 300..500, 600..1000
    assert_eq!(a.stats(), (700, 400));
    a.release(1);
    assert_eq!(a.stats(), (800, 700));
}

#[test]
fn a_device_without_a_dsp_has_no_area() {
    assert_eq!(Area::new(0, 0).stats(), (0, 0));
}

#[test]
fn a_range_starts_at_the_area_base_plus_its_offset() {
    let mut a = Area::new(0x2000_0100, 4096);
    a.take(3, 512, 128);
    assert_eq!(a.range_base(3), 0x2000_0300);
    assert_eq!(a.range_len(3), 128);
    assert_eq!(a.base(), 0x2000_0100);
}
