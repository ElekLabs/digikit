//! The `project` service as a service file (`services/project.dks`): the crate `dkrt-svc-project` behind `service!`, the five hooks of the stock project paths (`hooks.s`), the tick that drives the service (`glue::tick`) and the table that the runtime's settings overlay forwards to (`glue::SCOPE`).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use glue::Owner;

#[rustfmt::skip]
mod gen_exports;
pub mod glue;
pub mod hooks;

dkrt_svc_rt::service!(
    Owner,
    "project",
    &[],
    parts = [],
    hooks = [
        ("cf.project.hook.slot_set", hooks::project_hook_slot),
        ("cf.project.hook.save_exit", hooks::project_hook_save),
        (
            "cf.project.hook.clear_one_exit",
            hooks::project_hook_clear_one
        ),
        (
            "cf.project.hook.clear_many_exit",
            hooks::project_hook_clear_many
        ),
        ("cf.project.hook.paste_exit", hooks::project_hook_paste),
    ],
    fills = [],
    exports = gen_exports::exports(),
    tick = glue::tick
);
