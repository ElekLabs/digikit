//! The `lfo` service as a service file (`services/lfo.dks`): the crate `dkrt-svc-lfo` over the `dkc_*` imports (docs/plans/runtime-and-services.md section 13.2), the image's table of added
//! waves (`image_table.rs`) and the UI-task hooks of the MOD page (`hooks-ui.s`): the shape name and the tile vector of the graph (docs/plans/extension-model.md E6). The LFO tick runs no code of
//! the file. The WAVE maximum is no hook: the service writes the rows of the parameter table (its firmware words are `DKRT_LFO_FW` of `hooks-ui.s`, read by `Lfo::start`).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use dkrt_sdk::records::KINDS_LFO;
pub use image_table::Owner;

pub mod hooks;
pub mod image_table;

dkrt_svc_rt::service!(
    Owner,
    "lfo",
    &KINDS_LFO,
    parts = [Records],
    hooks = [
        ("cf.lfo.hook.wave_name", hooks::lfo_ui_name4),
        ("cf.lfo.hook.wave_name3", hooks::lfo_ui_name3, dn2),
        ("cf.lfo.hook.graph_vec", hooks::lfo_ui_graph),
    ],
    fills = []
);
