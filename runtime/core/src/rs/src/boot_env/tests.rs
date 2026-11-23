//! `RealBootEnv`'s package steps over the real arena (`TestArena`): what stage 1 placed goes back when the boot does not run the package.
use super::*;
use crate::arena;
use crate::fake_platform::test_arena::TestArena;
use dkrt_sdk::syspkg::body::{BodyHeader, HEADER_BYTES};

#[test]
fn abandoning_the_placed_package_returns_its_block_to_the_arena() {
    let _arena = TestArena::lock();
    unsafe {
        arena::init();
        let before = arena::stats();
        let env = RealBootEnv::new();
        let h = BodyHeader { entry: 0, image: 64, bss: 32, relocs: 0, align: 16 };
        let mut head = h.encode().to_vec();
        head.resize(HEADER_BYTES + 64, 0);
        env.placer.borrow_mut().begin((HEADER_BYTES + 64) as u32, &head).expect("the block fits the arena");
        assert_ne!(arena::stats(), before, "the package holds a block");
        // the block the tools audit: it starts where the arena gave it and covers the body (header, image, relocations or bss) up to the arena's 16 bytes
        let (lo, hi) = env.placer.borrow().extent().expect("a placed package has an extent");
        assert_eq!((lo & 15, hi - lo), (0, (HEADER_BYTES as u32 + 64 + 32 + 15) & !15));
        env.abandon_package();
        assert_eq!(arena::stats(), before, "the block is back");
        assert!(env.placer.borrow().extent().is_none(), "no block, no extent");
        env.abandon_package(); // nothing placed any more: harmless
        assert_eq!(arena::stats(), before);
    }
}
