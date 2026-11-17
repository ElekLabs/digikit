//! The `samples` service as a service file (`services/samples.dks`): the crate `dkrt-svc-samples` over the `dkc_*` imports, the nine `dk_sample_pool_*` exports
//! (`glue.rs`) and the two pool event slots the sample-pool adapter calls (`pool_done`, `pool_lost`). The adapter's calls come from its table `sample_pool_info`
//! (`pool::from_info`, installed by `glue::setup`); the file has no ColdFire hook (docs/plans/runtime-slimming.md, RS-S).
// a host build (the assembly test of the runtime) has std; the file is `no_std` on the ColdFire
#![cfg_attr(target_arch = "m68k", no_std)]

use dkrt_svc_samples::records::Samples;

pub mod glue;

dkrt_svc_rt::service!(
    Samples,
    "samples",
    // the service owns no record kind of the `register` section
    &[],
    setup = glue::setup,
    hooks = [],
    fills = [("pool_done", glue::pool_done_fill), ("pool_lost", glue::pool_lost_fill)],
    exports = [
        ("dk_sample_pool_open", glue::dk_sample_pool_open),
        ("dk_sample_pool_add", glue::dk_sample_pool_add),
        ("dk_sample_pool_clear", glue::dk_sample_pool_clear),
        ("dk_sample_pool_status", glue::dk_sample_pool_status),
        ("dk_sample_pool_info", glue::dk_sample_pool_info),
        ("dk_sample_pool_path", glue::dk_sample_pool_path),
        ("dk_sample_pool_set", glue::dk_sample_pool_set),
        ("dk_sample_pool_peaks", glue::dk_sample_pool_peaks),
        ("dk_sample_pool_peaks_window", glue::dk_sample_pool_peaks_window)
    ]
);
