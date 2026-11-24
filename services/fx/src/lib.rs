//! `fx` service: effect pages, FX parameters, routes and send amounts (docs/plans/runtime-and-services.md, R9).
//!
//! A crate of its own, linked into the service file `fx.dks`. It reaches the runtime only through `dkrt-abi` (`CoreApi`, interface C) and the SDK crate.
//! Every function that needs the core takes it by reference (`core: &C`, `C: CoreApi`): the file passes its `ImportCore` (a zero-sized type over the `dkc_*` imports) and a host test
//! passes a `FakeCore`.
//! Layout (README.md): `fx` (the owner `Fx` with `Provider`, `Ticker` and `Describe`), `records` (the `Records` part), `dsp/` (the `DspPart` and the effect rows' loader steps),
//! `routing` (the FX ROUTING list and its picks, over a small `Ui` trait), `builder` (the builder calls' collection), `rule` (the rule for the effect records), `slots` (docs only: the file fills no pointer slot, see `service-check`); `fxpage`, `fxsend` and `rows` are the tables the service keeps by name;
//! `tests/` (host tests over `FakeCore`).
#![no_std]
#[cfg(test)]
extern crate std;
pub mod builder;
pub mod dsp;
pub mod fx;
pub mod fxpage;
pub mod fxsend;
pub mod records;
pub mod routing;
pub mod rows;
pub mod rule;
pub mod slots;
#[cfg(test)]
pub(crate) mod test_lock;
