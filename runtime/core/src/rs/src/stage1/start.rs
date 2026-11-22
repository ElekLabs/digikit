use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::hooks::PackageHooks;
use dkrt_abi::mem::call_at;
use dkrt_abi::resident::{check, Refusal, Resident, ResidentTable};
use core::mem::MaybeUninit;
use dkrt_sdk::logk;

/// Would a package built against resident table `built_minor` accept `table`? The same `dkrt_abi::resident::check` the package runs in its entry (a table that is null, of
/// another magic or major, older than the package's minor or short of a group is refused), asked before the entry is called so that the refusal has a reason in the log
/// and the entry is never called with a table it would turn down.
///
/// # Safety
/// `table` is null or a live resident table, as `check` requires.
pub unsafe fn accept_table(table: *const ResidentTable, built_minor: u16) -> Result<(), Refusal> {
    let mut scratch = MaybeUninit::<Resident>::uninit();
    check(table, built_minor, scratch.as_mut_ptr())
}

/// Log why the package's table was refused: an error event whose text names the reason and whose value word holds what was found.
pub unsafe fn log_refusal(why: &Refusal) {
    crate::state::log_at(logk::L_ERROR as u32, why.message().as_ptr(), why.found);
}

/// Call the package's entry (`dkpkg_entry`, at `entry` once placed) with `table` and give its hooks; `None` (logged) when it answers null. The caller has asked `accept_table`
/// with the package's built minor, so a null answer is the package's own refusal, not a version mismatch.
///
/// # Safety
/// `entry` is the entry of a package that `ArenaPlacer::finish` made runnable, and `table` outlives the boot.
pub unsafe fn start(entry: DevAddr, table: &'static ResidentTable) -> Option<&'static PackageHooks> {
    let hooks = call_at(entry, &[table as *const ResidentTable as usize]) as *const PackageHooks;
    if hooks.is_null() {
        crate::state::log_at(logk::L_ERROR as u32, b"package: no hooks\0".as_ptr(), 0);
        return None;
    }
    Some(&*hooks)
}
