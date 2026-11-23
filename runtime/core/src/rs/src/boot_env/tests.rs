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
        env.abandon_package();
        assert_eq!(arena::stats(), before, "the block is back");
        env.abandon_package(); // nothing placed any more: harmless
        assert_eq!(arena::stats(), before);
    }
}
