//! The table of services the core walks (`ServiceTable`): one `ProviderHandle` per loaded service file, in the order the core calls them (callback order).
//! Every walk runs each row's matching method: the first refusal stops the walks that can refuse, and a part a provider did not adopt answers with its default
//! (accept, nothing).
use crate::core_api::CoreApi;
use crate::dsp::{DspSec, Job};
use crate::provider::ProviderHandle;
use crate::service::{ApplyCtx, CheckCtx, Claim, ModuleCommitCtx, RowWrite, SettleCtx, TableFault};
use core::ffi::c_void;
use dkrt_sdk::fault::DIAG_WORDS;
use dkrt_sdk::OK;

/// The most words one service writes for one consumer (the size of the buffer `ServiceTable::describe` gives each row; words past it are dropped).
pub const DESCRIBE_WORDS: usize = 16;

/// The table of services, in the order the core calls them: the walks below run the rows in table order (removal runs the other way). `core` is the `&C` that
/// each row's functions cast back.
#[derive(Clone, Copy)]
pub struct ServiceTable {
    pub rows: &'static [ProviderHandle],
    /// The core every call passes to the service (`&REAL_CORE` in the runtime's generated table).
    pub core: *const c_void,
}

// A table is read-only data (`rows` and `core` point at statics).
unsafe impl Sync for ServiceTable {}

impl ServiceTable {
    /// A table over `rows` that passes `core` to every method.
    #[inline]
    pub const fn new<C: CoreApi + 'static>(
        rows: &'static [ProviderHandle],
        core: &'static C,
    ) -> ServiceTable {
        ServiceTable {
            rows,
            core: core as *const C as *const c_void,
        }
    }
    /// The length of a record of `kind` (the whole record, header included) and the row that owns it; `None` when no service owns the kind.
    #[inline]
    fn owns(&self, kind: u32) -> Option<(ProviderHandle, u32)> {
        let mut i = 0;
        while i < self.rows.len() {
            let kinds = unsafe { self.rows[i].kinds() };
            let mut k = 0;
            while k < kinds.len() {
                if kinds[k].0 == kind {
                    return Some((self.rows[i], kinds[k].1));
                }
                k += 1;
            }
            i += 1;
        }
        None
    }
    /// The row that owns record kind `kind`.
    #[inline]
    pub fn owner(&self, kind: u32) -> Option<ProviderHandle> {
        self.owns(kind).map(|(row, _)| row)
    }
    /// The length of a record of `kind` (the whole record, header included); `None` when no service owns the kind.
    #[inline]
    pub fn len(&self, kind: u32) -> Option<u32> {
        self.owns(kind).map(|(_, len)| len)
    }
    #[inline]
    pub unsafe fn check(&self, step: u32, c: &CheckCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].check(self.core, step, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// `Provider::attach` of every row in table order.
    #[inline]
    pub unsafe fn apply(&self, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].attach(self.core, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// `Provider::attach` of every row in table order, for a consumer without records (`apply` is the walk of a consumer with records, which its caller undoes): the first refusal, after the providers that attached before it were detached
    /// (last first); the refusing provider and those after it are not detached, they never attached.
    #[inline]
    pub unsafe fn attach_providers(&self, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].attach(self.core, c);
            if r != OK {
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    self.rows[j].detach(self.core, c.owner);
                }
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn apply_record(&self, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].apply_record(self.core, kind, body, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// `Provider::detach` of every row; last service first.
    #[inline]
    pub unsafe fn remove(&self, owner: u8) {
        let mut i = self.rows.len();
        while i > 0 {
            i -= 1;
            self.rows[i].detach(self.core, owner);
        }
    }
    /// `(limit, used)` of capacity kind `kind`: the sum of the counts of the services that count it; `None` when none does.
    #[inline]
    pub unsafe fn cap(&self, kind: u8) -> Option<(u32, u32)> {
        let mut r: Option<(u32, u32)> = None;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some((l, u)) = self.rows[i].cap(self.core, kind) {
                r = Some(match r {
                    Some((a, b)) => (a + l, b + u),
                    None => (l, u),
                });
            }
            i += 1;
        }
        r
    }
    #[inline]
    pub unsafe fn attribute(&self, site: u32, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            let got = self.rows[i].attribute(self.core, site, id);
            if got.is_some() {
                return got;
            }
            i += 1;
        }
        None
    }
    /// The DSP diagnostics of every row, in table order.
    #[inline]
    pub unsafe fn diag(&self, words: &[u32; DIAG_WORDS]) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].diag(self.core, words);
            i += 1;
        }
    }
    /// What each service holds for consumer `owner` (a module record + 1): `visit(name hash, words)` for every row of the table, in table order, that adopts the describe part
    /// and wrote at least one word (at most `DESCRIBE_WORDS` of them). The core reads no meaning into the words. Runs in the caller's task: a row's describe never enters its owner.
    #[inline]
    pub unsafe fn describe(&self, owner: u8, mut visit: impl FnMut(u32, &[u32])) {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].has_describe() {
                let mut words = [0u32; DESCRIBE_WORDS];
                let n = self.rows[i].describe(self.core, owner, &mut words);
                if n > 0 {
                    visit(self.rows[i].name(), &words[..n]);
                }
            }
            i += 1;
        }
    }
    /// The check of the services' tables over the rows in table order: the first number that is not 0 (the mismatch is in `out`), else 0.
    #[inline]
    pub unsafe fn table_check(&self, used: u32, out: &mut TableFault) -> u32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].table_check(self.core, used, out);
            if r != 0 {
                return r;
            }
            i += 1;
        }
        0
    }
    #[inline]
    pub unsafe fn on_loaded(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].on_loaded(self.core, rec);
            i += 1;
        }
    }
    /// `settle` on every row, in table order.
    #[inline]
    pub unsafe fn settle(&self, s: &SettleCtx) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].settle(self.core, s);
            i += 1;
        }
    }
    /// `Provider::can_stop` on every row, in table order: the first status that is not `OK`, else `OK`.
    #[inline]
    pub unsafe fn can_stop(&self) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].can_stop(self.core);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// `Provider::reload_done` on every row, in table order.
    #[inline]
    pub unsafe fn reload_done(&self) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].reload_done(self.core);
            i += 1;
        }
    }
    /// `module_begin` on every row, in table order.
    #[inline]
    pub unsafe fn module_begin(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].module_begin(self.core, rec);
            i += 1;
        }
    }
    /// `module_commit` over the rows in table order; the first refusal stops the walk and is the result.
    #[inline]
    pub unsafe fn module_commit(&self, rec: usize, c: &ModuleCommitCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].module_commit(self.core, rec, c);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn tick(&self, phase: u32) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].tick(self.core, phase);
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_check_sections(&self, sec: &DspSec, job: &mut Job) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].dsp_check_sections(self.core, sec, job);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_check_rows(&self, sec: &DspSec) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].dsp_check_rows(self.core, sec);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_check_types(&self, sec: &DspSec) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].dsp_check_types(self.core, sec);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_admit(&self, job: &Job) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = self.rows[i].dsp_admit(self.core, job);
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn dsp_claim(&self, c: &mut Claim) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].dsp_claim(self.core, c);
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_release(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].dsp_release(self.core, rec);
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_clear(&self, rec: usize) -> bool {
        let mut ok = true;
        let mut i = 0;
        while i < self.rows.len() {
            let cleared = self.rows[i].dsp_clear(self.core, rec);
            ok = cleared && ok;
            i += 1;
        }
        ok
    }
    #[inline]
    pub unsafe fn dsp_row_step(&self, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        let mut i = 0;
        while i < self.rows.len() {
            let got = self.rows[i].dsp_row_step(self.core, rec, want, write);
            if got.is_some() {
                return got;
            }
            i += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests;
