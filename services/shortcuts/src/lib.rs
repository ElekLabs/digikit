//! `shortcuts` service: key combinations that modules bind with `dk_shortcut_add` (docs/plans/track-manager.md).
//!
//! A neutral crate, loaded as the service file `dks/` makes of it. It reaches the runtime only through `dkrt-abi` (`CoreApi`, interface C) and the SDK crate: every
//! function that needs the core takes it by reference (`core: &C`, `C: CoreApi`), so a host test passes a `FakeCore` and the runtime passes its `RealCore`.
//! The crate exports only `dk_module` (the entry that carries its `ProviderRow`); it has no assembler and names no firmware (that is `dks/`).
//!
//! Owner objects, each with private state:
//! - [`keys::KeyState`]: which logical keys are held.
//! - [`bindings::BindingTable`]: the bindings of all modules, by owner; the first owner of a key pair keeps the binding.
//! - [`builder::Builder`]: the collection of the builder call `dk_shortcut_add` while a module's `init` runs.
//! - [`shortcuts::Shortcuts`]: the owner of the service, one instance in the file's cell. It holds the three above, takes the key events (`on_key`: [`shortcuts::Verdict`]),
//!   and implements `Provider` (lifecycle) and `Ticker` (the UI pass).
//!
//! Modules: [`builder`] (the builder call and the commit that writes the records), [`records`] (the `Records` part, and [`records::validate`], the entry that
//! `patchkit records --check` calls), [`rule`] (the rule for the records of one module, `check_set`, and the record layout).
#![no_std]
#[cfg(test)]
extern crate std;
pub mod bindings;
pub mod builder;
pub mod keys;
pub mod records;
pub mod rule;
pub mod shortcuts;
