//! `IntroFns`: the boot screen.
use crate::intro;
use dkrt_abi::resident::{Group, IntroFns};

unsafe extern "C" fn commit(rec: u32) {
    intro::commit(rec as usize)
}
unsafe extern "C" fn discard(rec: u32) {
    intro::discard(rec as usize)
}

pub(super) static FNS: Group<IntroFns> = Group::new(IntroFns { commit, discard });
