//! `machines` service: run-time machine types, parameters, page clicks, native pages and the voice rows of the DSP section (docs/plans/runtime-and-services.md, R10).
//!
//! A crate of its own, linked into the runtime object. It reaches the runtime only through `dkrt-abi`: `CoreApi` (interface C: the registry and DSP loader
//! state, the type map, the parameter rows, the native page calls), and `Fw` (the firmware's parameter page, for a click's flow). Every function that needs
//! one takes it by reference (`core: &C`, `fw: &impl Fw`): the runtime passes its `RealCore` and `RuntimeFw` (zero-sized types, so the calls are static) and a
//! host test passes a `FakeCore` and a `FakeFw`. The click state (`slots`) and the state between `apply` and `apply_record` (`records`) are statics of
//! this crate: one machines service, one set. The runtime's hooks (`dkrt_page_key`, `dkrt_page_turn`) are thin wrappers in `handlers.rs`.
//! Layout (the service template, `sdk/templates/service`): `records` (the `Service` handlers and the state between `apply` and `apply_record`), `check/` (the load check),
//! `apply` (the rows of a module), `unload` and `parking` (give rows back; absent numbers), `registry` (the registry state), `slots/` (the click state; host tests in
//! `slots/tests.rs`), `build/` (the builder calls and their commit), `assign` (sticky type numbers), `guard` (the machine guard's diagnostics), `modules` (the MODULES reply),
//! `validate` (the record rules), `rows` (the voice rows), `info` (the image table), `device_profile` (the device's profile), `selection_view` (the machine selection view),
//! `nativepage`, `paramapi` and `dsp/` (the service's own modules). Tests that cross the crate are in `tests/`.
#![no_std]
#[cfg(test)]
extern crate std;

pub mod apply;
pub mod assign;
pub mod build;
pub mod check;
pub mod device_profile;
pub mod dsp;
pub mod guard;
pub mod info;
pub mod modules;
pub mod nativepage;
pub mod paramapi;
pub mod parking;
pub mod records;
pub mod registry;
pub mod rows;
pub mod selection_view;
pub mod slots;
pub mod unload;
pub mod validate;

pub(crate) use device_profile::profile;
