//! The `lfo` service as a service file (`services/lfo.dks`): the crate `dkrt-svc-lfo` over the `dkc_*` imports (docs/plans/runtime-and-services.md section 13.2), and the six interrupt
//! hooks of the LFO tick and its second reader (`hooks-dt2.s`, `hooks-dn2.s`), through which the tick reads the shape table, and the UI-task hooks of the MOD page (`hooks-ui.s`): the shape name and the tile vector of the graph (docs/plans/extension-model.md E6). The WAVE maximum is no hook: the service writes the rows of the parameter table (its firmware words are `DKRT_LFO_FW` of `hooks-ui.s`, read by `Lfo::start`).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use dkrt_sdk::records::KINDS_LFO;
use dkrt_svc_lfo::lfo::Lfo;

mod hooks;

dkrt_svc_rt::service!(
    Lfo,
    "lfo",
    &KINDS_LFO,
    parts = [Records],
    hooks = [
        ("cf.lfo.hook.tick_call", hooks::lfo_tick_call_a, dt2),
        ("cf.lfo.hook.tick_slew", hooks::lfo_tick_slew_a, dt2),
        ("cf.lfo.hook.tick_start", hooks::lfo_tick_start_a, dt2),
        ("cf.lfo.hook.read_call", hooks::lfo_read_call_a, dt2),
        ("cf.lfo.hook.read_slew", hooks::lfo_read_slew_a, dt2),
        ("cf.lfo.hook.read_start", hooks::lfo_read_start_a, dt2),
        ("cf.lfo.hook.tick_call", hooks::lfo_tick_call_b, dn2),
        ("cf.lfo.hook.tick_slew", hooks::lfo_tick_slew_b, dn2),
        ("cf.lfo.hook.tick_start", hooks::lfo_tick_start_b, dn2),
        ("cf.lfo.hook.read_call", hooks::lfo_read_call_b, dn2),
        ("cf.lfo.hook.read_slew", hooks::lfo_read_slew_b, dn2),
        ("cf.lfo.hook.read_start", hooks::lfo_read_start_b, dn2),
        ("cf.lfo.hook.wave_name", hooks::lfo_ui_name4),
        ("cf.lfo.hook.wave_name3", hooks::lfo_ui_name3, dn2),
        ("cf.lfo.hook.graph_vec", hooks::lfo_ui_graph),
    ],
    fills = []
);
