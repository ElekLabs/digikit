//! `check`: does a package accept a resident table? One function, shared by `install` (the package, today) and stage 1 (later).
use super::{Reason, Refusal, Resident, ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR};

/// Accept the table and copy its groups into `into`, or say why not (`into` is then untouched): the magic is equal, the major is equal, the minor is at least `built_minor` (the minor the package was built against), the table
/// holds a pointer for every group the package knows and each group is large enough for the entries the package uses. Never panics; the caller logs the reason.
///
/// # Safety
/// `into` is valid for writes of a `Resident`. `table` and the groups it points at are live and readable, as the producer built them (an artifact's table is read at its address; a null pointer is
/// the one invalid pointer the check handles).
pub unsafe fn check(
    table: *const ResidentTable,
    built_minor: u16,
    into: *mut Resident,
) -> Result<(), Refusal> {
    if table.is_null() {
        return Err(Refusal::new(Reason::NoTable, 0, 0));
    }
    // the header first: a table of another version may be shorter than ours
    let h = core::ptr::read(core::ptr::addr_of!((*table).header));
    if h.magic != RESIDENT_MAGIC {
        return Err(Refusal::new(Reason::Magic, h.magic, RESIDENT_MAGIC));
    }
    if h.major != RESIDENT_MAJOR {
        return Err(Refusal::new(
            Reason::Major,
            h.major as u32,
            RESIDENT_MAJOR as u32,
        ));
    }
    if h.minor < built_minor {
        return Err(Refusal::new(
            Reason::Minor,
            h.minor as u32,
            built_minor as u32,
        ));
    }
    let need = core::mem::size_of::<ResidentTable>() as u32;
    if h.size < need {
        return Err(Refusal::new(Reason::TableSize, h.size, need));
    }
    Resident::open(&*table, into)
}
