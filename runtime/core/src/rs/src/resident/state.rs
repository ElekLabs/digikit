//! `StateFns`: the resident state. The entry is the owner's own `extern "C"` function.
use dkrt_abi::resident::{Group, StateFns};

pub(super) static FNS: Group<StateFns> = Group::new(StateFns { init_once: crate::state::init_once });
