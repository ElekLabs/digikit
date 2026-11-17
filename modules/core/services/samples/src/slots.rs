//! `samples` service, slots and hooks: it fills the two pool event slots of the sample-pool adapter (`pool_done`, `pool_lost`: the routines `pool_done_fill` and
//! `pool_lost_fill` of the service file, `dks/src/glue.rs`) and installs no hook. `dkrt_project_changed` had no caller and is gone.
