//! The ColdFire side of the DSP hosts, a package part of `system.dksp` (`[package_part]` of `core/dsp-link`). The part is found through the table `DKPART_TABLE`
//! (`dkrt_abi::part`): the core names no part. One owner type for each concept, one file each:
//!
//! - `area`: the DSP area and its first-fit ranges;
//! - `relocation`: a module's DSP image rebased to the range it got;
//! - `transfer`: the stream of an image and of word writes through the link queue (`link_queue`, a seam with a fake);
//! - `voice_host_table`: the `voice_host` info table (`dkrt_sdk::voice_host`), magic 0 until the host serves;
//! - `host`: the part's state, and `part`: its table.
#![cfg_attr(not(test), no_std)]

mod area;
mod host;
mod link_queue;
mod part;
mod relocation;
mod transfer;
mod voice_host_table;

pub use area::Area;
pub use link_queue::LinkQueue;
pub use relocation::{Malformed, Relocation};
pub use transfer::{Advance, Job, Rows, Staging, Transfer};
pub use voice_host_table::{Offer, VoiceHostTable};
