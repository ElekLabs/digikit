//! `IntroFns`: the boot screen (`intro.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IntroFns {
    pub commit: unsafe extern "C" fn(rec: u32),
    pub discard: unsafe extern "C" fn(rec: u32),
}
