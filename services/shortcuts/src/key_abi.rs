//! `KeyAbi`: the numbers of `core/input`'s key subscribers that the service shares with the image: the magic and version of `input_keys_info` and the answers of `key_subscribe`. The
//! one source is the header of the image's module (`runtime/platform/input/src/cf/key_subscribers.h`); these are the one Rust copy of its `#define`s (a build of the service stages
//! only its crates, so the header cannot be read at compile time), and `tests/key_header.rs` fails when the header and this file differ. No other Rust file names the numbers.

/// `KEYS_INFO_MAGIC` (`'KEYS'`): word 0 of `input_keys_info`.
pub const MAGIC: u32 = 0x4b45_5953;
/// `KEYS_INFO_VERSION`: word 1.
pub const VERSION: u32 = 1;
/// `KEYS_FULL`: `key_subscribe` found every slot taken.
pub const FULL: i32 = -1;
/// `KEYS_DUPLICATE`: `key_subscribe` found the routine subscribed already.
pub const DUPLICATE: i32 = -2;
