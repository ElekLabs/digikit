//! Canvas clipping and the overlay mask.
use super::*;
use crate::native_page::fake_firmware::*;

#[test]
fn canvas_clips_and_flips_to_the_bitmap() {
    let mut clip = Clip::default();
    let mut mask = [0u32; MASK_WORDS];
    let mut c = Canvas::new(&mut clip, 99, false, &mut mask);
    c.fill_rect(10, 20, 5, 2, 1); // swapped corners
    c.fill_rect(-5, -5, 200, 100, 7); // clamps, color becomes 1
    c.fill_rect(99, 0, 120, 5, 1); // outside
    c.fill_rect(0, 53, 5, 60, 1); // outside
    c.text(-1, 0, b"x");
    c.text(0, 53, b"x");
    c.text(3, 4, b"hello\0world");
    assert_eq!(clip.fills, [[5, 52 - 20, 10, 52 - 2, 1], [0, 0, 98, 52, 1]]);
    assert_eq!(clip.texts, [(3, 48, b"hello".to_vec())]);
}

#[test]
fn overlay_mask_matches_the_c_header() {
    let hash = |m: &[u32]| fnv(m.iter().flat_map(|w| w.to_le_bytes()));
    let mut mask = [0u32; 99 * 2];
    for (a, b) in [(0, 22), (24, 47), (76, 98)] {
        mask_rect(&mut mask, 99, 53, a, 0, b, 19);
    }
    assert_eq!(hash(&mask), 0x459438c9);
    let visible = |x: usize, y: usize| {
        let row = 52 - y;
        (mask[x * 2 + row / 32] >> (31 - row % 32)) & 1
    };
    for x in 0..99usize {
        for y in 0..53usize {
            let want = y <= 19 && (x <= 22 || (24..=47).contains(&x) || x >= 76);
            assert_eq!(visible(x, y) == 1, want, "({x},{y})");
        }
    }
    mask_rect(&mut mask, 99, 53, -10, -10, 200, 100);
    assert_eq!(hash(&mask), 0xcc2e4403);
    for x in 0..99 {
        assert_eq!((mask[x * 2], mask[x * 2 + 1]), (u32::MAX, u32::MAX << 11));
    }
}
