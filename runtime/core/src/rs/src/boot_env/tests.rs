//! `RealBootEnv`'s package steps over the real package block (`TestArena` maps it): what stage 1 placed is forgotten when the boot does not run the package, and the arena is never touched.
use super::*;
use crate::arena;
use crate::fake_platform::test_arena::TestArena;
use dkrt_sdk::syspkg::body::{BodyHeader, HEADER_BYTES};

#[test]
fn abandoning_the_placed_package_empties_its_extent_and_leaves_the_arena_whole() {
    let _arena = TestArena::lock();
    unsafe {
        arena::init();
        let before = arena::stats();
        let env = RealBootEnv::new();
        let h = BodyHeader { entry: 0, image: 64, bss: 32, relocs: 0, align: 16 };
        let mut head = h.encode().to_vec();
        head.resize(HEADER_BYTES + 64, 0);
        env.placer.borrow_mut().begin((HEADER_BYTES + 64) as u32, &head).expect("the block fits the package block");
        assert_eq!(arena::stats(), before, "the package takes none of the arena");
        // the block the tools audit: it starts at the package block and covers the body (header, image, relocations or bss) up to 16 bytes
        let (lo, hi) = env.placer.borrow().extent().expect("a placed package has an extent");
        assert_eq!((lo, hi - lo), (crate::state::package_block().0.get(), (HEADER_BYTES as u32 + 64 + 32 + 15) & !15));
        env.abandon_package();
        assert!(env.placer.borrow().extent().is_none(), "no block, no extent");
        env.abandon_package(); // nothing placed any more: harmless
        assert_eq!(arena::stats(), before);
    }
}
