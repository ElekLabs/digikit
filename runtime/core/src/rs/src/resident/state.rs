//! `StateFns`: the resident state.
use dkrt_abi::resident::StateFns;

unsafe extern "C" fn init_once() {
    crate::state::init_once()
}

pub(super) static FNS: StateFns = StateFns { init_once };
