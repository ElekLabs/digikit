//! The page dots against the C header.
use super::*;
use crate::native_page::fake_firmware::fnv;

#[test]
fn dots_match_the_c_header() {
    let golden = [0x62b95591u32, 0x00f09591, 0xbcc31dc5];
    for (page, want) in golden.iter().enumerate() {
        let mut pix = [[0u8; 128]; 64];
        dots(page as u32, |x0, y0, x1, y1, c| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    pix[y as usize][x as usize] = (c != 0) as u8;
                }
            }
        });
        assert_eq!(fnv(pix.iter().flatten().copied()), *want, "page {page}");
    }
    // the C test: page 0 is the upper dot, page 1 the lower, page 2 nothing
    let count = |page: u32, first: i32, last: i32| {
        let mut n = 0;
        dots(page, |x0, y0, x1, y1, _| {
            for y in y0..=y1 {
                n += (x1 - x0 + 1) * (first..=last).contains(&(63 - y)) as i32;
            }
        });
        n
    };
    assert!(count(0, 28, 33) > count(0, 35, 41));
    assert!(count(1, 28, 33) < count(1, 35, 41));
    assert_eq!(count(2, 0, 63), 0);
}
