//! `IntroFns`: the boot screen.
use crate::intro;
use dkrt_abi::resident::IntroFns;

unsafe extern "C" fn commit(rec: u32) {
    intro::commit(rec as usize)
}
unsafe extern "C" fn discard(rec: u32) {
    intro::discard(rec as usize)
}

pub(super) static FNS: IntroFns = IntroFns { commit, discard };
