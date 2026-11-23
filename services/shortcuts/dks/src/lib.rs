//! The `shortcuts` service as a service file: the `service!` call and the exports (`gen_exports.rs`, the builder call `dk_shortcut_add` in `builder.rs`). The key-dispatch hook is added with the
//! key hook (hooks.rs, hooks.s).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use dkrt_sdk::records::KINDS_SHORTCUTS;
use dkrt_svc_shortcuts::shortcuts::Shortcuts;

pub mod builder;
#[rustfmt::skip]
mod gen_exports;
pub mod hooks;

dkrt_svc_rt::service!(
    Shortcuts,
    "shortcuts",
    &KINDS_SHORTCUTS,
    parts = [Records, Tick],
    hooks = [("cf.input.hook.key_event", hooks::shortcuts_key_hook)],
    fills = [],
    exports = gen_exports::exports()
);
