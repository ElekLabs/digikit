//! The `fx` service as a service file (`services/fx.dks`): the `service!` call and nothing else. The nine hooks of the effect pages are assembler in `hooks.rs`; the builder calls of the effects are the exports of `gen_exports.rs`, written by `patchkit gen`.
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]
// `global_asm!` on ColdFire is an experimental architecture of the compiler
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use dkrt_sdk::records::KINDS_FX;
use dkrt_svc_fx::records::Fx;

#[rustfmt::skip]
mod gen_exports;
mod hooks;
mod rt;
pub use rt::dk_service_query_v1;

dkrt_svc_rt::service!(
    Fx,
    "fx",
    &KINDS_FX,
    setup = hooks::setup,
    hooks = [
        ("cf.page.hook.page_desc", hooks::fxp_acc),
        ("cf.page.hook.tab_label_test", hooks::fxp_label),
        ("cf.page.hook.filter_graph_test", hooks::fxp_graph),
        ("cf.page.hook.fx_view_list.load", hooks::fxp_list),
        ("cf.page.hook.fx_array.load", hooks::fxp_load),
        ("cf.page.hook.fx_array.save", hooks::fxp_save),
        ("cf.page.hook.fx_array.init_exit", hooks::fxp_init),
        ("cf.page.hook.fx_key_list.load", hooks::fxp_keylist),
        ("cf.page.hook.sound_load_exit", hooks::fxp_sound),
    ],
    fills = [],
    exports = gen_exports::exports(),
    query = rt::dk_service_query_v1
);
