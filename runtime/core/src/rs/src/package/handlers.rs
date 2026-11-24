//! Interface B of docs/ARCHITECTURE.md: what the core calls on a service, and only what the service declared.
//!
//! A service is a provider: the core keeps one table of the rows of the loaded service files (`provider_rows.rs`, in the room `gen_services.rs` gives it) and walks it through
//! `ServiceTable`: a row is the service's name hash, its record kinds and its part tables over an owner object; no `dyn`. The core names no service outside the table.
//!
//! What a service declares:
//! - record kinds of the `register` section (the records part: kind and length from sdk.toml), and their `check`, `apply` and `remove` (the registry walker calls them);
//! - the DSP section parts it owns: `dsp_check_*`, `dsp_admit`, `dsp_claim`, `dsp_release`, `dsp_clear`, `dsp_row_step`
//!   (the DSP loader in `dsp/mod.rs` keeps the area, the row table and the state machine);
//! - capacity counts (`cap`, per capacity kind), the module behind a fault site (`attribute`);
//! - `on_loaded` (every row of a module is in the DSP).
pub use dkrt_abi::service::*;

/// The table over the runtime's own core.
pub type ServiceTable = dkrt_abi::service::ServiceTable;

/// The services in the order the core calls them: the rows of the loaded service files (`provider_rows.rs`).
pub fn services() -> ServiceTable {
    #[cfg(test)]
    crate::package::provider_rows::test_machines();
    crate::package::provider_rows::table()
}

/// Host tests only: what the runtime's own tests of the record path take from the service crates (the type numbers of the machines service for the module being loaded, its
/// row as a loaded service file has it, the shapes of the lfo service).
#[cfg(test)]
pub(crate) mod test_seams {
    pub(crate) use dkrt_svc_lfo::records::{MAX_SHAPES, SAMPLES};
    pub(crate) use dkrt_svc_lfo::shapes::{owner_of, shape, used};
    pub(crate) use dkrt_svc_machines::assign::seed_for_test;
    pub(crate) fn machines_row() -> crate::package::provider_rows::RowSource<'static> {
        crate::package::handlers::dks_decls::machines_source()
    }
    /// The voice rows of module record `rec` (a bit mask) as a DSP load would leave them; `None` forgets every voice row.
    pub(crate) fn set_voice_rows(rec: usize, mask: Option<u32>) {
        match mask {
            Some(m) => {
                dkrt_svc_machines::rows::vrows().mod_rows[rec] = m;
                // what a DSP load does when it is complete: the MODULES reply reads the published entry
                dkrt_svc_machines::records::publish(&dkrt_test_support::FakeCore::new());
            }
            None => dkrt_svc_machines::rows::reset(),
        }
    }
}

/// The length of a record of `kind`; `E_FORMAT`-worthy kinds are `None`. For the builder lowering (`collect`).
pub fn len(kind: u32) -> Option<u32> {
    services().len(kind)
}

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
pub(crate) mod lfo_tests;
#[cfg(test)]
pub(crate) mod fx_fixture;
#[cfg(test)]
pub(crate) mod machines_fixture;
#[cfg(test)]
pub(crate) mod dks_decls;
