//! The table of services the core walks (`ServiceTable`): one row per service, in the order the core calls them. A row is either a `ServiceRow` of the old `Service`
//! trait (an in-crate service, or a service file that has not moved to the provider contract) or a [`ProviderHandle`] (a service file's `ProviderRow`, resolved).
//! Every walk runs each row's matching method and keeps the old semantics: the first refusal stops the walks that can refuse, and a part a provider did not adopt
//! answers with the default of the old method. RC3 deletes the `Service` arm with the old trait.
use crate::core_api::CoreApi;
use crate::dsp::{DspSec, Job};
use crate::provider::ProviderHandle;
use crate::service::{
    ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, RowWrite, ServiceRow, SettleCtx,
    TableFault,
};
use core::ffi::c_void;
use dkrt_sdk::fault::DIAG_WORDS;
use dkrt_sdk::OK;

/// One position of the table.
#[derive(Clone, Copy)]
pub enum Row {
    /// A row of the old `Service` trait.
    Service(*const ServiceRow),
    /// A provider file's row, resolved against the part versions the core knows, with its instance.
    Provider(ProviderHandle),
}

impl Row {
    /// The record kinds the row owns (kind, whole record length).
    ///
    /// # Safety
    /// The row's pointers are valid.
    pub unsafe fn kinds(&self) -> &'static [(u32, u32)] {
        match self {
            Row::Service(p) => (**p).kinds(),
            Row::Provider(h) => h.kinds(),
        }
    }

    /// The hash of the service's name (`dkneeds::keys::name_hash`): the identity a row is found by.
    ///
    /// # Safety
    /// The row's pointers are valid.
    pub unsafe fn name_hash(&self) -> u32 {
        match self {
            Row::Service(p) => crate::provider::name_hash((**p).name()),
            Row::Provider(h) => h.name(),
        }
    }
}

/// The rows of a table, in a static (a raw pointer is not `Sync`; the rows are read-only).
#[repr(transparent)]
pub struct RowPtrs<const N: usize>(pub [Row; N]);

unsafe impl<const N: usize> Sync for RowPtrs<N> {}

impl<const N: usize> RowPtrs<N> {
    /// The rows over `rows`, in order.
    pub const fn of(rows: &'static [ServiceRow; N]) -> RowPtrs<N> {
        let mut a = [Row::Service(core::ptr::null::<ServiceRow>()); N];
        let mut i = 0;
        while i < N {
            a[i] = Row::Service(&rows[i] as *const ServiceRow);
            i += 1;
        }
        RowPtrs(a)
    }
}

/// The rows over `rows` followed by empty entries up to `M` entries (`M >= N`): the table of a core that adds the rows of loaded service files after its own.
pub const fn rows_with_tail<const N: usize, const M: usize>(
    rows: &'static [ServiceRow; N],
) -> [Row; M] {
    let mut a = [Row::Service(core::ptr::null::<ServiceRow>()); M];
    let mut i = 0;
    while i < N {
        a[i] = Row::Service(&rows[i] as *const ServiceRow);
        i += 1;
    }
    a
}

/// The table of services, in the order the core calls them: the walks below run the rows in table order (removal runs the other way). It is the
/// same type for every core; `core` is the `&C` that each row's thunks cast back.
#[derive(Clone, Copy)]
pub struct ServiceTable {
    pub rows: &'static [Row],
    /// Bit `i` set: row `i` FAILED its declare check (`check_declarations`); its kinds are unknown to `owner` and `len`.
    pub failed: u32,
    /// The core every call passes to the service (`&REAL_CORE` in the runtime's generated table).
    pub core: *const c_void,
}

// A table is read-only data (`rows` and `core` point at statics).
unsafe impl Sync for ServiceTable {}

impl ServiceTable {
    /// A table over `rows` that passes `core` to every method.
    #[inline]
    pub const fn new<C: CoreApi + 'static>(rows: &'static [Row], core: &'static C) -> ServiceTable {
        ServiceTable {
            rows,
            failed: 0,
            core: core as *const C as *const c_void,
        }
    }
    /// The old row at position `i`, `None` for a provider.
    #[inline(always)]
    fn old(&self, i: usize) -> Option<&'static ServiceRow> {
        match self.rows[i] {
            Row::Service(p) => Some(unsafe { &*p }),
            Row::Provider(_) => None,
        }
    }
    /// Declare-check-commit of the in-crate services: each old row's `declare` against its table (the generated static `kinds`). Returns the mask of the
    /// rows whose declarations differ (bit `i`: row `i`); the caller keeps it in the table it hands out (`failed`). A provider declares in its file's `init`, which
    /// the loader compares with the header table; it has nothing to check here.
    #[inline]
    pub unsafe fn check_declarations(&self) -> u32 {
        let mut failed = 0u32;
        let mut i = 0;
        while i < self.rows.len() && i < 32 {
            if let Some(row) = self.old(i) {
                let mut d = Declare::new();
                (row.declare)(self.core, &mut d);
                if !d.matches_kinds(row.kinds()) {
                    failed |= 1 << i;
                }
            }
            i += 1;
        }
        failed
    }
    /// Whether row `i` owns record kind `kind`.
    #[inline]
    fn owns(&self, i: usize, kind: u32) -> Option<u32> {
        if i < 32 && self.failed & (1 << i) != 0 {
            return None;
        }
        let kinds = unsafe { self.rows[i].kinds() };
        let mut k = 0;
        while k < kinds.len() {
            if kinds[k].0 == kind {
                return Some(kinds[k].1);
            }
            k += 1;
        }
        None
    }
    /// The row that owns record kind `kind`.
    #[inline]
    pub fn owner(&self, kind: u32) -> Option<Row> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.owns(i, kind).is_some() {
                return Some(self.rows[i]);
            }
            i += 1;
        }
        None
    }
    /// The length of a record of `kind` (the whole record, header included); `None` when no service owns the kind.
    #[inline]
    pub fn len(&self, kind: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(len) = self.owns(i, kind) {
                return Some(len);
            }
            i += 1;
        }
        None
    }
    #[inline]
    pub unsafe fn check(&self, step: u32, c: &CheckCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).check)(self.core, step, c),
                Row::Provider(h) => h.check(self.core, step, c),
            };
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// The old `apply` (a provider's `attach`) of every row in table order.
    #[inline]
    pub unsafe fn apply(&self, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).apply)(self.core, c),
                Row::Provider(h) => h.attach(self.core, c),
            };
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// `Provider::attach` of every provider row in table order, for a consumer without records (an old row has nothing to apply then): the first refusal, after the
    /// providers that attached before it were detached (last first); the refusing provider and those after it are not detached, they never attached.
    #[inline]
    pub unsafe fn attach_providers(&self, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            if let Row::Provider(h) = self.rows[i] {
                let r = h.attach(self.core, c);
                if r != OK {
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        if let Row::Provider(done) = self.rows[j] {
                            done.detach(self.core, c.owner);
                        }
                    }
                    return r;
                }
            }
            i += 1;
        }
        OK
    }
    #[inline]
    pub unsafe fn apply_record(&self, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).apply_record)(self.core, kind, body, c),
                Row::Provider(h) => h.apply_record(self.core, kind, body, c),
            };
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// The old `remove` (a provider's `detach`) of every row; last service first.
    #[inline]
    pub unsafe fn remove(&self, owner: u8) {
        let mut i = self.rows.len();
        while i > 0 {
            i -= 1;
            match self.rows[i] {
                Row::Service(p) => ((*p).remove)(self.core, owner),
                Row::Provider(h) => h.detach(self.core, owner),
            }
        }
    }
    /// `(limit, used)` of capacity kind `kind`: the sum of the counts of the services that count it; `None` when none does.
    #[inline]
    pub unsafe fn cap(&self, kind: u8) -> Option<(u32, u32)> {
        let mut r: Option<(u32, u32)> = None;
        let mut i = 0;
        while i < self.rows.len() {
            let got = match self.rows[i] {
                Row::Service(p) => {
                    let mut lu = [0u32; 2];
                    if ((*p).cap)(self.core, kind, lu.as_mut_ptr()) {
                        Some((lu[0], lu[1]))
                    } else {
                        None
                    }
                }
                Row::Provider(h) => h.cap(self.core, kind),
            };
            if let Some((l, u)) = got {
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
            let got = match self.rows[i] {
                Row::Service(p) => {
                    let mut m = 0usize;
                    if ((*p).attribute)(self.core, site, id, &mut m) {
                        Some(m)
                    } else {
                        None
                    }
                }
                Row::Provider(h) => h.attribute(self.core, site, id),
            };
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
            match self.rows[i] {
                Row::Service(p) => ((*p).diag)(self.core, words.as_ptr()),
                Row::Provider(h) => h.diag(self.core, words),
            }
            i += 1;
        }
    }
    /// The check of the services' tables over the rows in table order: the first number that is not 0 (the mismatch is in `out`), else 0.
    #[inline]
    pub unsafe fn table_check(&self, used: u32, out: &mut TableFault) -> u32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).table_check)(self.core, used, out),
                Row::Provider(h) => h.table_check(self.core, used, out),
            };
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
            match self.rows[i] {
                Row::Service(p) => ((*p).on_loaded)(self.core, rec),
                Row::Provider(h) => h.on_loaded(self.core, rec),
            }
            i += 1;
        }
    }
    /// `settle` on every row, in table order.
    #[inline]
    pub unsafe fn settle(&self, s: &SettleCtx) {
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows[i] {
                Row::Service(p) => ((*p).settle)(self.core, s),
                Row::Provider(h) => h.settle(self.core, s),
            }
            i += 1;
        }
    }
    /// The old `teardown_refusal` (a provider's `can_stop`, which names no consumer) on every row, in table order: the first status that is not `OK`, else `OK`.
    #[inline]
    pub unsafe fn teardown_refusal(&self, owner: u8) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).teardown_refusal)(self.core, owner),
                Row::Provider(h) => h.can_stop(self.core),
            };
            if r != OK {
                return r;
            }
            i += 1;
        }
        OK
    }
    /// The old `live_done` (a provider's `reload_done`) on every row, in table order.
    #[inline]
    pub unsafe fn live_done(&self) {
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows[i] {
                Row::Service(p) => ((*p).live_done)(self.core),
                Row::Provider(h) => h.reload_done(self.core),
            }
            i += 1;
        }
    }
    /// `module_begin` on every row, in table order.
    #[inline]
    pub unsafe fn module_begin(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows[i] {
                Row::Service(p) => ((*p).module_begin)(self.core, rec),
                Row::Provider(h) => h.module_begin(self.core, rec),
            }
            i += 1;
        }
    }
    /// `module_commit` over the rows in table order; the first refusal stops the walk and is the result.
    #[inline]
    pub unsafe fn module_commit(&self, rec: usize, c: &ModuleCommitCtx) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).module_commit)(self.core, rec, c),
                Row::Provider(h) => h.module_commit(self.core, rec, c),
            };
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
            match self.rows[i] {
                Row::Service(p) => ((*p).tick)(self.core, phase),
                Row::Provider(h) => h.tick(self.core, phase),
            }
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_check_sections(&self, sec: &DspSec, job: &mut Job) -> i32 {
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).dsp_check_sections)(self.core, sec, job),
                Row::Provider(h) => h.dsp_check_sections(self.core, sec, job),
            };
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
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).dsp_check_rows)(self.core, sec),
                Row::Provider(h) => h.dsp_check_rows(self.core, sec),
            };
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
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).dsp_check_types)(self.core, sec),
                Row::Provider(h) => h.dsp_check_types(self.core, sec),
            };
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
            let r = match self.rows[i] {
                Row::Service(p) => ((*p).dsp_admit)(self.core, job),
                Row::Provider(h) => h.dsp_admit(self.core, job),
            };
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
            match self.rows[i] {
                Row::Service(p) => ((*p).dsp_claim)(self.core, c),
                Row::Provider(h) => h.dsp_claim(self.core, c),
            }
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_release(&self, rec: usize) {
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows[i] {
                Row::Service(p) => ((*p).dsp_release)(self.core, rec),
                Row::Provider(h) => h.dsp_release(self.core, rec),
            }
            i += 1;
        }
    }
    #[inline]
    pub unsafe fn dsp_clear(&self, rec: usize) -> bool {
        let mut ok = true;
        let mut i = 0;
        while i < self.rows.len() {
            let cleared = match self.rows[i] {
                Row::Service(p) => ((*p).dsp_clear)(self.core, rec),
                Row::Provider(h) => h.dsp_clear(self.core, rec),
            };
            ok = cleared && ok;
            i += 1;
        }
        ok
    }
    #[inline]
    pub unsafe fn dsp_row_step(&self, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        let mut i = 0;
        while i < self.rows.len() {
            let got = match self.rows[i] {
                Row::Service(p) => {
                    let mut w = RowWrite {
                        win: 0,
                        word: 0,
                        value: 0,
                        published: None,
                        last: false,
                        has_undo: false,
                        undo: 0,
                    };
                    if ((*p).dsp_row_step)(self.core, rec, want, write, &mut w) {
                        Some(w)
                    } else {
                        None
                    }
                }
                Row::Provider(h) => h.dsp_row_step(self.core, rec, want, write),
            };
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
