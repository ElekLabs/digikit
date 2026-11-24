//! Interface B of docs/ARCHITECTURE.md: what the core calls on a service, and only what the service declared.
//!
//! A service is a provider: the core keeps one table of the rows of the loaded service files (`provider_rows.rs`, in the room `gen_services.rs` gives it) and walks it through
//! `ServiceTable`: a row is the service's name hash, its record kinds and its part tables over an owner object; no `dyn`. The core names a service outside the table in two
//! export lookups only (`exports`).
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

/// The C-ABI entry points of the services that the core links into its own tables (interface A: the imports of `sdk.toml` and the events the adapters
/// call). This is the one place where the core names a service function; the rest of the core calls `Services` or `CoreApi`.
pub mod exports {
    use dkrt_abi::service::ModuleMachines;

    /// The exports of the service files that only the runtime calls (`internal` imports of sdk/sdk.toml): the runtime looks them up by name in the loader's
    /// export table and keeps its behaviour when the service file is not loaded.
    pub const FX_ROUTES_ENTRY: &str = "dk_fx_routes_entry";
    pub const MACHINES_MODULE_INFO: &str = "dk_machines_module_info";

    /// Host tests: a fake export, by name hash, for the function that the table does not hold as a ColdFire address.
    #[cfg(test)]
    pub(crate) static TEST_EXPORTS: std::sync::Mutex<std::vec::Vec<(u32, usize)>> = std::sync::Mutex::new(std::vec::Vec::new());

    /// The address of the export `name` of a loaded service file: `None` when no loaded file publishes it.
    pub fn find_export(name: &str) -> Option<usize> {
        let hash = dkneeds::keys::name_hash(name);
        #[cfg(test)]
        {
            let fake = TEST_EXPORTS.lock().unwrap_or_else(|e| e.into_inner());
            let mut i = 0;
            while i < fake.len() {
                if fake[i].0 == hash {
                    return Some(fake[i].1);
                }
                i += 1;
            }
        }
        crate::package::loader::find_export(hash).map(|a| a as usize)
    }

    /// The effect list of DIGIKIT > FX ROUTES: the `fx` service file's export; without the file (no fx) there is no effect.
    pub unsafe fn fx_entry(index: u32, out: &mut dkrt_sdk::DkrtFx) -> i32 {
        match find_export(FX_ROUTES_ENTRY) {
            Some(a) => core::mem::transmute::<usize, unsafe extern "C" fn(u32, *mut u8) -> i32>(a)(index, out as *mut dkrt_sdk::DkrtFx as *mut u8),
            None => dkrt_sdk::E_NOT_FOUND,
        }
    }
    /// The machines of module record `owner` (`module_cmds`: the module list): the `machines` service file's export; none without the file.
    pub unsafe fn module_info(owner: u8) -> Option<ModuleMachines> {
        let a = find_export(MACHINES_MODULE_INFO)?;
        let mut m = ModuleMachines::default();
        if core::mem::transmute::<usize, unsafe extern "C" fn(u32, *mut u8) -> u32>(a)(owner as u32, &mut m as *mut ModuleMachines as *mut u8) != 0 {
            Some(m)
        } else {
            None
        }
    }
    /// Host tests only: the type numbers of the machines service for the module being loaded, and its row as a loaded service file has it (`provider_rows::test_machines`).
    #[cfg(test)]
    pub(crate) use dkrt_svc_machines::assign::seed_for_test;
    #[cfg(test)]
    pub(crate) fn machines_row() -> crate::package::provider_rows::RowSource<'static> {
        crate::package::handlers::dks_decls::machines_source()
    }
    /// The shapes of the lfo service, for the runtime's own tests of the record path.
    #[cfg(test)]
    pub(crate) use dkrt_svc_lfo::records::{MAX_SHAPES, SAMPLES};
    #[cfg(test)]
    pub(crate) use dkrt_svc_lfo::shapes::{owner_of, shape, used};
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
