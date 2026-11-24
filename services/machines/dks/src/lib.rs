//! The `machines` service as a service file (`services/machines.dks`): the crate `dkrt-svc-machines` over the `dkc_*` imports, the hooks of the machine framework (`hooks.rs`),
//! the click flow's firmware calls and the exports `dk_param_get` and `dk_param_set` (`fw.rs`), the two builder calls that take a callback (`builder.rs`), the list of all exports (`gen_exports.rs`, written by `patchkit gen`); the image table `machines_info` is bound by `Provider::start` of the owner (`info::bind`)
//! (docs/plans/runtime-and-services.md section 13.2, R12-6).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use dkrt_sdk::records::KINDS_MACHINES;
use dkrt_svc_machines::machines::Machines;

pub mod builder;
pub mod fw;
#[rustfmt::skip]
mod gen_exports;
mod guard;
mod hooks;
pub mod native;

dkrt_svc_rt::service!(
    Machines,
    "machines",
    &KINDS_MACHINES,
    parts = [Records, Dsp, Tick, Describe],
    hooks = [
        ("cf.machine.hook.clone.5", hooks::mf_clone5, dt2),
        ("cf.machine.hook.param_code.args", hooks::mf_typearg, dt2),
        ("cf.machine.hook.srcpage", hooks::mf_srcpage, dt2),
        ("cf.machine.hook.list_rank_insert", hooks::mf_rank, dt2),
        ("cf.machine.hook.group", hooks::mf_group, dn2),
        ("cf.machine.hook.type_page.count", hooks::mf_typemap0, dn2),
        ("cf.machine.hook.type_page.desc", hooks::mf_typemap1, dn2),
        ("cf.machine.hook.param_code.map", hooks::mf_typemap2, dn2),
        ("cf.machine.hook.type_reader.load", hooks::mf_reader, dn2),
        ("cf.machine.hook.owner_type_load", hooks::mf_owner, dn2),
        ("cf.machine.hook.frametype", hooks::mf_frametype, dn2),
        ("cf.page.hook.source_draw", hooks::mf_draw_hook),
        ("cf.page.hook.source_code", hooks::mf_code_hook),
        ("cf.page.hook.source_key", hooks::mf_key_hook),
        ("cf.page.hook.source_turn", hooks::mf_turn_hook),
    ],
    fills = [],
    patches = [
        ("cf.machine.hook.patch_list_alloc", 2, Machines::list_bytes),
        (
            "cf.machine.hook.patch_list_vector_end",
            2,
            Machines::list_bytes
        ),
        (
            "cf.machine.hook.patch_list_buffer_end",
            4,
            Machines::list_end
        ),
        ("cf.machine.hook.patch_list_buffer", 4, Machines::list_start),
    ],
    exports = gen_exports::exports()
);
