//! The boot steps of the system package: the header tables of the selection, the providers they need and the package manager's load. `boot::run` calls them through
//! `BootEnv`.

use crate::package::resident::store;
use crate::package::{lifecycle, loader};
use dkrt_sdk::sel::Record;
use dkrt_sdk::OK;

/// Read the file of each selected module and keep its header table for the boot's graph. A file that is missing, torn or not a `.dkm` has no table (its load reports the error).
pub(crate) unsafe fn read_tables(rec: &Record) {
    loader::reset_plan();
    let mut k = 0;
    while k < rec.count as usize {
        if let Ok(found) = store::find(rec.paths[k].as_ptr()) {
            if store::load_to_stage(&found) == OK {
                let file = core::slice::from_raw_parts(store::stage() as *const u8, found.size as usize);
                loader::capture(k, file);
            }
        }
        k += 1;
    }
}

/// Search the card for the providers the selection `count` modules long needs, from the kept tables.
pub(crate) unsafe fn find_providers(count: usize) {
    loader::find_providers(count)
}

/// The package manager's load of the selection.
pub(crate) unsafe fn load(rec: &Record) {
    lifecycle::boot(rec)
}
