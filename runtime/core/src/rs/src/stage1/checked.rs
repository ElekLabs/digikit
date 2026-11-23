use super::*;
use dkrt_abi::resident::ResidentTable;

impl<S: PackageSource, P: Placer> Stage1<'_, S, P> {
    /// Stage 1 as the boot runs it: `load`, and then the resident table must fit the package before its entry is called (`accept_table`, built with the minor the package
    /// was built against). A table that does not fit has its reason in the log (`log_refusal`), the block is free again (`abandon`) and the status is
    /// `Incompatible`: a card state, never a crash. Any other status is `load`'s.
    ///
    /// # Safety
    /// `table` is null or a live resident table, as `accept_table` requires.
    pub unsafe fn load_checked(&mut self, table: *const ResidentTable) -> PackageStatus {
        let status = self.load();
        let Some((_, built_minor)) = self.placed() else { return status };
        if let Err(why) = accept_table(table, built_minor) {
            log_refusal(&why);
            self.abandon();
            return PackageStatus::Incompatible;
        }
        status
    }
}
