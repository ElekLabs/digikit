//! The owner object of the service file: the service over a `KeyEvents`. On the ColdFire it is the image's table of key subscribers (`image_keys.rs`); a host build (the tests of the file,
//! the assembly test of the runtime) has no image, so its owner subscribes to `UnwiredKeys`.
#[cfg(target_arch = "m68k")]
pub use crate::image_keys::Owner;

#[cfg(not(target_arch = "m68k"))]
pub type Owner =
    dkrt_svc_shortcuts::shortcuts::Shortcuts<dkrt_svc_shortcuts::unwired_keys::UnwiredKeys>;
