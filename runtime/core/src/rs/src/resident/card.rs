//! `CardFns`: the card. The entry is the owner's own `extern "C"` function.
use dkrt_abi::resident::{CardFns, Group};

pub(super) static FNS: Group<CardFns> = Group::new(CardFns { writable: crate::card::writable });
