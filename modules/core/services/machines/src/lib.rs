//! `machines` service: run-time machine types, parameters, page clicks, native pages and the voice rows of the DSP section (docs/plans/runtime-and-services.md, R10).
//!
//! A crate of its own, linked into the runtime object. It reaches the runtime only through `dkrt-abi`: `CoreApi` (interface C: the registry and DSP loader
//! state, the type map, the parameter rows, the native page calls), and `Fw` (the firmware's parameter page, for a click's flow). Every function that needs
//! one takes it by reference (`core: &C`, `fw: &impl Fw`): the runtime passes its `RealCore` and `RuntimeFw` (zero-sized types, so the calls are static) and a
//! host test passes a `FakeCore` and a `FakeFw`. The click state (`slots`) and the state between `apply` and `apply_record` (`records`) are statics of
//! this crate: one machines service, one set. The runtime's hooks (`dkrt_page_key`, `dkrt_page_turn`) are thin wrappers in `handlers.rs`.
//! Layout (the service template, `sdk/templates/service`): `records` (the `Service` handlers), `slots` `tests/` (host tests over `FakeCore`); `nativepage`, `paramapi` and `dsp/` are the service's own modules.
#![no_std]
#[cfg(test)]
extern crate std;
/// The profile of the device the runtime runs on, by the device id (the generated table `dkrt_abi::profiles`): the run-time machine types of the device.
/// `None` for an id without a profile.
pub(crate) fn profile<C: dkrt_abi::core_api::CoreApi>(
    core: &C,
) -> Option<dkrt_abi::platform::DeviceProfile> {
    dkrt_abi::platform::DeviceProfile::by_id(core.device_id())
}

pub mod assign;
pub mod build;
pub mod dsp;
pub mod guard;
pub mod info;
pub mod nativepage;
pub mod paramapi;
pub mod records;
pub mod rows;
pub mod slots;
pub mod validate;
