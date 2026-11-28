//! The `shortcuts` service as a service file: the `service!` call and the exports (`gen_exports.rs`, the builder call `dk_shortcut_add` in `builder.rs`). The key routine (`key_routine.rs`) takes no
//! firmware site: the service subscribes it to the key walk of `core/input` when it starts (`image_keys.rs`).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]

use dkrt_sdk::records::KINDS_SHORTCUTS;

pub mod builder;
#[rustfmt::skip]
mod gen_exports;
#[cfg(target_arch = "m68k")]
mod image_keys;
pub mod key_routine;

/// The owner object of the service file: the service over the image's table of key subscribers.
#[cfg(target_arch = "m68k")]
pub use image_keys::Owner;
/// A host build (the tests of the file, the assembly test of the runtime) has no image: the owner subscribes to a `FakeKeys`.
#[cfg(not(target_arch = "m68k"))]
pub type Owner = dkrt_svc_shortcuts::shortcuts::Shortcuts<dkrt_svc_shortcuts::fake_keys::FakeKeys>;

dkrt_svc_rt::service!(
    Owner,
    "shortcuts",
    &KINDS_SHORTCUTS,
    parts = [Records, Tick],
    hooks = [],
    fills = [],
    exports = gen_exports::exports()
);
