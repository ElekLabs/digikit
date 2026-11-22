//! `StateFns`: the resident state (`state.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StateFns {
    /// First-use setup of the runtime (the arena's free list, the magic): once per boot.
    pub init_once: unsafe extern "C" fn(),
}
