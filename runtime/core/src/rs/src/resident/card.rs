//! `CardFns`: the card.
use dkrt_abi::resident::CardFns;

extern "C" fn writable() -> bool {
    crate::card::writable()
}

pub(super) static FNS: CardFns = CardFns { writable };
