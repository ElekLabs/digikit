//! `shortcuts` service: key combinations that modules bind with `dk_shortcut_add` (docs/plans/track-manager.md).
//!
//! A neutral crate, loaded as the service file `dks/` makes of it. It reaches the runtime only through `dkrt-abi` (`CoreApi`, interface C) and the SDK crate: every
//! function that needs the core takes it by reference (`core: &C`, `C: CoreApi`), so a host test passes a `FakeCore` and the runtime passes its `RealCore`.
//! The crate exports only `dk_module` (the entry that carries its `ServiceRow`) and `DKRT_*` state; it has no assembler and names no firmware (that is `dks/`).
//!
//! Owner objects, each with private state:
//! - [`keys::KeyState`]: which logical keys are held.
//! - [`bindings::BindingTable`]: the bindings of all modules, by owner; the first owner of a key pair keeps the binding.
//! - [`shortcuts::Shortcuts`]: the state machine. `on_key` takes one key event and says whether the stock firmware may see it ([`shortcuts::Verdict`]).
//!
//! Modules: [`build`] (the builder call `dk_shortcut_add` and the commit that writes the records), [`records`] (the `Service` handlers), [`validate`] (the rule for the
//! records, shared with `patchkit records --check`), [`slots`] (none yet: the key-dispatch hook comes later).
#![no_std]
#[cfg(test)]
extern crate std;
pub mod bindings;
pub mod build;
pub mod keys;
pub mod records;
pub mod shortcuts;
pub mod slots;
pub mod validate;
