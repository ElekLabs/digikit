//! `machines` service: run-time machine types, parameters, page clicks, native pages and the voice rows of the DSP section (docs/plans/runtime-and-services.md, R10).
//!
//! A crate of its own, linked into the runtime object. It reaches the runtime only through `dkrt-abi`: `CoreApi` (interface C: the registry and DSP loader
//! state, the type map, the parameter rows, the native page calls), and `Fw` (the firmware's parameter page, for a click's flow). Every function that needs
//! one takes it by reference (`core: &C`, `fw: &impl Fw`): the runtime passes its `RealCore` and `RuntimeFw` (zero-sized types, so the calls are static) and a
//! host test passes a `FakeCore` and a `FakeFw`. The click state, the collection of the builder calls, the parking passes and the state between `attach` and `apply` belong to the owner `Machines`; the
//! registry, the numbers, the voice rows and the guard are tables the service keeps by name (README.md). The runtime's hooks (`dkrt_page_key`, `dkrt_page_turn`) are thin wrappers in `handlers.rs`.
//! Layout (README.md): `machines` (the owner `Machines` with `Provider`, `Ticker` and `Describe`, and the state the old statics held), `records` (the `Records` part and the free
//! `validate`), `dsp/` (the `DspPart` and the voice rows' loader steps), `apply` (`attach` and the per-record `apply`: the rows of a module), `check/` (the load check),
//! `unload` and `parking` (give rows back; absent numbers), `registry` (the registry state), `slots/` (the click state; host tests in `slots/tests.rs`), `build/` (the builder
//! calls and their commit), `assign` (sticky type numbers), `guard` (the machine guard's diagnostics), `modules` (the MODULES reply), `validate` (the record rules), `rows` (the
//! voice rows), `info` (the image table), `device_profile` (the device's profile), `list` (the machine list and rank tables), `selection_view` (the machine selection view), `nativepage`, `pagerows` (the free page rows at the start), `native_page/` (the native page logic and the page table the owner holds), `paramapi`. Tests that cross the
//! crate are in `tests/`.
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
pub mod list;
pub mod machines;
pub mod modules;
pub mod native_page;
pub mod nativepage;
pub mod pagerows;
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
