use super::*;
use crate::fake_platform::test_arena::TestArena;

const HDR_BYTES: u32 = HDR;

#[test]
fn the_top_block_leaves_the_addresses_below_as_they_were() {
    let _arena = TestArena::lock();
    unsafe {
        init();
        let first = alloc(64);
        free(first);
        let top = alloc_top(1000);
        assert!(!top.is_null());
        assert_eq!(top.get() & 15, 0);
        // the block ends at the top of the arena, and the first allocation still lands where it did
        assert_eq!(top.get() + ((1000 + 15) & !15), st().arena.hi);
        assert_eq!(alloc(64), first);
    }
}

#[test]
fn only_one_top_block_fits_and_it_is_given_back_whole() {
    let _arena = TestArena::lock();
    unsafe {
        init();
        let before = stats();
        let top = alloc_top(4096);
        assert!(!top.is_null());
        let (free_now, _) = stats();
        assert_eq!(before.0 - free_now, 4096 + HDR_BYTES);
        // the top is taken: a second top block is refused
        assert!(alloc_top(16).is_null());
        assert!(free(top));
        assert_eq!(stats(), before);
        assert!(!alloc_top(16).is_null());
    }
}

#[test]
fn a_top_block_larger_than_the_arena_or_of_no_size_is_refused() {
    let _arena = TestArena::lock();
    unsafe {
        init();
        let before = stats();
        assert!(alloc_top(0).is_null());
        assert!(alloc_top(ARENA_BYTES as u32 + 1).is_null());
        assert_eq!(stats(), before);
    }
}

#[test]
fn a_top_block_takes_the_whole_free_block_when_too_little_would_stay() {
    let _arena = TestArena::lock();
    unsafe {
        init();
        let (_, largest) = stats();
        let top = alloc_top(largest);
        assert!(!top.is_null());
        assert_eq!(stats(), (0, 0));
        assert!(free(top));
        assert_eq!(stats().1, largest);
    }
}
