//! `lfo` service: the shapes an LFO is read through, in an owner-tracked table (docs/plans/runtime-and-services.md, R7 and R12-1).
//!
//! A crate of its own, linked into the runtime object. It reaches the runtime only through `dkrt-abi` (`CoreApi`, interface C) and the SDK crate: the
//! service never names the runtime. The runtime names `Lfo` once, in the generated service table (`gen_services.rs`). Layout: [`lfo`] (the owner object `Lfo<T>`: `Provider`), [`graph`] (the tiles the MOD page draws for a shape), [`records`] (the
//! `lfo_shape` records and their handlers), [`shapes`] (the owner's table of shapes and the lookups over it), [`wave_table`] (the image's table of added waves, behind a trait), [`wave`] (the WAVE maximum in the parameter table), [`route`] (the table index of a wave number, tied to the bounds in the UI hook assembler), [`validate`] (the rule every shape module meets, shared with `patchkit records --lfo-check`), `tests/` (host tests over `FakeCore`).
#![no_std]
#[cfg(test)]
extern crate std;
pub mod graph;
pub mod lfo;
pub mod records;
pub mod route;
pub mod shapes;
pub mod validate;
pub mod wave;
pub mod wave_table;
