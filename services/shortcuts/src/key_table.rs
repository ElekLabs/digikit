//! `KeyTable`: the words of the image's `input_keys_info` (`core/input`, `runtime/platform/input/src/cf/key_subscribers.h`) as the service reads them. The service finds the table by the
//! hash of its name (`CoreApi::info`) and calls the two entries it holds.
use crate::key_abi;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;
use dkrt_sdk::{E_EXISTS, E_NO_REGISTRY, E_TABLE_FULL};

/// The words of `input_keys_info`: magic, version, slots, subscribe, unsubscribe.
pub const WORDS: usize = 5;
/// The name of the image's data table (`[[info]]` of `core/input`).
const NAME: &str = "input_keys_info";

/// The table the words describe: how many routines it holds, and the addresses of its two calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyTable {
    slots: u32,
    subscribe: u32,
    unsubscribe: u32,
}

impl KeyTable {
    /// The table the words describe, or none: another magic or version, no slot, or a missing entry.
    pub fn parse(w: &[u32; WORDS]) -> Option<KeyTable> {
        if w[0] != key_abi::MAGIC || w[1] != key_abi::VERSION || w[2] == 0 || w[3] == 0 || w[4] == 0
        {
            return None;
        }
        Some(KeyTable {
            slots: w[2],
            subscribe: w[3],
            unsubscribe: w[4],
        })
    }

    /// The table of the image `core` runs, or none when the image has no `input_keys_info` (stock, or patched without `core/input` on this device).
    pub fn find<C: CoreApi>(core: &C) -> Option<KeyTable> {
        // SAFETY: the image's table is `WORDS` words of read-only data
        let t = unsafe { core.info(dkneeds::keys::name_hash(NAME)) };
        if t.is_null() {
            return None;
        }
        let mut w = [0u32; WORDS];
        let mut i = 0;
        while i < WORDS {
            // SAFETY: as above
            w[i] = unsafe { t.add(i).read() };
            i += 1;
        }
        KeyTable::parse(&w)
    }

    /// The routines the table holds at once.
    pub fn slots(&self) -> u32 {
        self.slots
    }

    /// The entry of `int32_t key_subscribe(key_subscriber fn)`.
    pub fn subscribe(&self) -> u32 {
        self.subscribe
    }

    /// The entry of `int32_t key_unsubscribe(key_subscriber fn)`.
    pub fn unsubscribe(&self) -> u32 {
        self.unsubscribe
    }

    /// The result of `key_subscribe`: the slot the routine took (0 or more) is `Ok`; a full table, a routine subscribed already, or any other answer is the status that fails the start.
    pub fn subscribe_result(answer: i32) -> Result<(), Status> {
        match answer {
            a if a >= 0 => Ok(()),
            key_abi::FULL => Err(E_TABLE_FULL),
            key_abi::DUPLICATE => Err(E_EXISTS),
            _ => Err(E_NO_REGISTRY),
        }
    }
}
