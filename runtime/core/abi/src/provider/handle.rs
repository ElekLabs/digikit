use super::row::{check, CheckedRow, PartLookup};
use super::tables::*;
use super::{
    ProviderRow, Status, PART_DESCRIBE, PART_DSP, PART_LIFECYCLE, PART_QUERY, PART_RECORDS,
    PART_STATE, PART_TICK,
};
use crate::dsp::{DspSec, Job};
use crate::service::{ApplyCtx, CheckCtx, Claim, ModuleCommitCtx, RowWrite, SettleCtx, TableFault};
use core::ffi::c_void;
use core::mem::{align_of, size_of};
use core::ops::Range;
use dkrt_sdk::fault::DIAG_WORDS;
use dkrt_sdk::{E_FORMAT, E_SVC_ABI, OK};

/// A service row the core read from a file, resolved against the part versions it knows, with the instance its functions work on: what the core keeps for one loaded
/// provider and calls through. A part the service did not adopt answers with its default (accept, nothing). The pointers stay valid from link until the package is freed: the owner of the handle drops it first.
#[derive(Clone, Copy)]
pub struct ProviderHandle {
    name: u32,
    instance: *mut c_void,
    lifecycle: *const LifecycleV1,
    records: *const RecordsV1,
    dsp: *const DspV1,
    tick: *const TickV1,
    query: *const QueryV1,
    describe: *const DescribeV1,
    state: *const StateV1,
}

/// The table of a part that is inside `extent` and aligned, or the refusal.
fn table_of<T>(found: PartLookup, extent: &Range<usize>) -> Result<*const T, Status> {
    match found {
        PartLookup::Missing => Ok(core::ptr::null()),
        PartLookup::TooNew { .. } => Err(E_SVC_ABI),
        PartLookup::Found { table, .. } => {
            let at = table as usize;
            let end = at.checked_add(size_of::<T>());
            if at % align_of::<T>() != 0 || at < extent.start || end.is_none_or(|e| e > extent.end)
            {
                return Err(E_FORMAT);
            }
            Ok(table as *const T)
        }
    }
}

impl ProviderHandle {
    /// The handle of no service: the filler of the unused cells of a row table. Nothing walks it (a walk covers the filled prefix), and a call through it would fault.
    pub const EMPTY: ProviderHandle = ProviderHandle {
        name: 0,
        instance: core::ptr::null_mut(),
        lifecycle: core::ptr::null(),
        records: core::ptr::null(),
        dsp: core::ptr::null(),
        tick: core::ptr::null(),
        query: core::ptr::null(),
        describe: core::ptr::null(),
        state: core::ptr::null(),
    };

    /// Resolve `row` (read from a package that lies in `extent`) for `instance`. `E_FORMAT` for a row, a table or a record kinds array outside `extent` or a null instance, `E_SVC_ABI` for a
    /// part whose version is newer than this core knows.
    ///
    /// # Safety
    /// As [`check`]: `extent` is readable; the handle is dropped before the package is freed. `instance` is what the row's functions work on.
    pub unsafe fn resolve(
        row: &ProviderRow,
        extent: Range<usize>,
        instance: *mut c_void,
    ) -> Result<ProviderHandle, Status> {
        if instance.is_null() {
            return Err(E_FORMAT);
        }
        let row: CheckedRow<'_> = check(row, extent.clone())?;
        let lifecycle =
            table_of::<LifecycleV1>(row.part(PART_LIFECYCLE, LIFECYCLE_NEWEST), &extent)?;
        let records = table_of::<RecordsV1>(row.part(PART_RECORDS, RECORDS_NEWEST), &extent)?;
        if !records.is_null() && unsafe { (*records).nkinds } != 0 {
            // SAFETY: `table_of` checked that the table lies inside `extent`, which the caller guarantees readable.
            let (kinds, n) = unsafe { ((*records).kinds, (*records).nkinds as usize) };
            let at = kinds as usize;
            let len = n.checked_mul(size_of::<(u32, u32)>());
            let end = len.and_then(|l| at.checked_add(l));
            if kinds.is_null()
                || at % align_of::<(u32, u32)>() != 0
                || at < extent.start
                || end.is_none_or(|e| e > extent.end)
            {
                return Err(E_FORMAT);
            }
        }
        Ok(ProviderHandle {
            name: row.name(),
            instance,
            lifecycle,
            records,
            dsp: table_of(row.part(PART_DSP, DSP_NEWEST), &extent)?,
            tick: table_of(row.part(PART_TICK, TICK_NEWEST), &extent)?,
            query: table_of(row.part(PART_QUERY, QUERY_NEWEST), &extent)?,
            describe: table_of(row.part(PART_DESCRIBE, DESCRIBE_NEWEST), &extent)?,
            state: table_of(row.part(PART_STATE, STATE_NEWEST), &extent)?,
        })
    }

    /// The name hash of the service.
    pub fn name(&self) -> u32 {
        self.name
    }

    /// The instance the functions work on.
    pub fn instance(&self) -> *mut c_void {
        self.instance
    }

    /// The record kinds the service owns: (kind, whole record length). None without a records part.
    ///
    /// # Safety
    /// The handle's tables are valid.
    pub unsafe fn kinds(&self) -> &'static [(u32, u32)] {
        match self.records.as_ref() {
            Some(r) => r.kinds(),
            None => &[],
        }
    }

    // ---- the lifecycle part

    /// `Provider::attach`: the consumer's section (`c.reg`, null for none) and image base.
    pub unsafe fn attach(&self, core: *const c_void, c: &ApplyCtx) -> i32 {
        ((*self.lifecycle).attach)(self.instance, core, c)
    }
    /// `Provider::detach` for owner id `owner`.
    pub unsafe fn detach(&self, core: *const c_void, owner: u8) {
        ((*self.lifecycle).detach)(self.instance, core, owner)
    }
    /// `Provider::can_stop`.
    pub unsafe fn can_stop(&self, core: *const c_void) -> i32 {
        ((*self.lifecycle).can_stop)(self.instance, core)
    }
    pub unsafe fn settle(&self, core: *const c_void, s: &SettleCtx) {
        ((*self.lifecycle).settle)(self.instance, core, s)
    }
    pub unsafe fn reload_done(&self, core: *const c_void) {
        ((*self.lifecycle).reload_done)(self.instance, core)
    }

    // ---- the records part

    pub unsafe fn module_begin(&self, core: *const c_void, rec: usize) {
        if let Some(r) = self.records.as_ref() {
            (r.begin)(self.instance, core, rec)
        }
    }
    pub unsafe fn module_commit(
        &self,
        core: *const c_void,
        rec: usize,
        c: &ModuleCommitCtx,
    ) -> i32 {
        match self.records.as_ref() {
            Some(r) => (r.commit)(self.instance, core, rec, c),
            None => OK,
        }
    }
    pub unsafe fn check(&self, core: *const c_void, step: u32, c: &CheckCtx) -> i32 {
        match self.records.as_ref() {
            Some(r) => (r.check)(self.instance, core, step, c),
            None => OK,
        }
    }
    /// One record of the consumer's section: a kind the service does not own is `OK` and not passed on.
    pub unsafe fn apply_record(
        &self,
        core: *const c_void,
        kind: u32,
        body: *const u8,
        c: &ApplyCtx,
    ) -> i32 {
        let Some(r) = self.records.as_ref() else {
            return OK;
        };
        match r.kinds().iter().find(|k| k.0 == kind) {
            Some(&(_, len)) => (r.apply)(self.instance, core, kind, body, len.saturating_sub(8), c),
            None => OK,
        }
    }

    // ---- the DSP part

    pub unsafe fn dsp_check_sections(
        &self,
        core: *const c_void,
        sec: &DspSec,
        job: &mut Job,
    ) -> i32 {
        match self.dsp.as_ref() {
            Some(d) => (d.check_sections)(self.instance, core, sec, job),
            None => OK,
        }
    }
    pub unsafe fn dsp_check_rows(&self, core: *const c_void, sec: &DspSec) -> i32 {
        match self.dsp.as_ref() {
            Some(d) => (d.check_rows)(self.instance, core, sec),
            None => OK,
        }
    }
    pub unsafe fn dsp_check_types(&self, core: *const c_void, sec: &DspSec) -> i32 {
        match self.dsp.as_ref() {
            Some(d) => (d.check_types)(self.instance, core, sec),
            None => OK,
        }
    }
    pub unsafe fn dsp_admit(&self, core: *const c_void, job: &Job) -> i32 {
        match self.dsp.as_ref() {
            Some(d) => (d.admit)(self.instance, core, job),
            None => OK,
        }
    }
    pub unsafe fn dsp_claim(&self, core: *const c_void, c: &mut Claim) {
        if let Some(d) = self.dsp.as_ref() {
            (d.claim)(self.instance, core, c)
        }
    }
    pub unsafe fn dsp_release(&self, core: *const c_void, rec: usize) {
        if let Some(d) = self.dsp.as_ref() {
            (d.release)(self.instance, core, rec)
        }
    }
    pub unsafe fn dsp_clear(&self, core: *const c_void, rec: usize) -> bool {
        match self.dsp.as_ref() {
            Some(d) => (d.clear)(self.instance, core, rec),
            None => true,
        }
    }
    pub unsafe fn dsp_row_step(
        &self,
        core: *const c_void,
        rec: usize,
        want: &mut u32,
        write: bool,
    ) -> Option<RowWrite> {
        let d = self.dsp.as_ref()?;
        let mut w = RowWrite {
            win: 0,
            word: 0,
            value: 0,
            published: None,
            last: false,
            has_undo: false,
            undo: 0,
        };
        (d.row_step)(self.instance, core, rec, want, write, &mut w).then_some(w)
    }
    pub unsafe fn on_loaded(&self, core: *const c_void, rec: usize) {
        if let Some(d) = self.dsp.as_ref() {
            (d.loaded)(self.instance, core, rec)
        }
    }
    pub unsafe fn attribute(&self, core: *const c_void, site: u32, id: u32) -> Option<usize> {
        let d = self.dsp.as_ref()?;
        let mut m = 0usize;
        (d.attribute)(self.instance, core, site, id, &mut m).then_some(m)
    }
    pub unsafe fn diag(&self, core: *const c_void, words: &[u32; DIAG_WORDS]) {
        if let Some(d) = self.dsp.as_ref() {
            (d.diag)(self.instance, core, words.as_ptr())
        }
    }

    // ---- the tick part

    pub unsafe fn tick(&self, core: *const c_void, phase: u32) {
        if let Some(t) = self.tick.as_ref() {
            (t.tick)(self.instance, core, phase)
        }
    }

    // ---- the query part

    /// The resident query entry's call (`ResidentQueryV1`): `E_NOT_FOUND`-like refusal is the service's; without a query part `E_MISSING`.
    pub unsafe fn query(&self, op: u32, req: &[u8], reply: &mut [u8]) -> i32 {
        match self.query.as_ref() {
            Some(q) => (q.query)(
                self.instance,
                op,
                req.as_ptr(),
                req.len() as u32,
                reply.as_mut_ptr(),
                reply.len() as u32,
            ),
            None => dkrt_sdk::E_MISSING,
        }
    }

    // ---- the describe part

    pub unsafe fn cap(&self, core: *const c_void, kind: u8) -> Option<(u32, u32)> {
        let d = self.describe.as_ref()?;
        let mut lu = [0u32; 2];
        (d.cap)(self.instance, core, kind, lu.as_mut_ptr()).then_some((lu[0], lu[1]))
    }
    pub unsafe fn table_check(&self, core: *const c_void, used: u32, out: &mut TableFault) -> u32 {
        match self.describe.as_ref() {
            Some(d) => (d.table_check)(self.instance, core, used, out),
            None => 0,
        }
    }
    /// Does the service adopt the describe part?
    pub fn has_describe(&self) -> bool {
        !self.describe.is_null()
    }
    /// The words the service holds for consumer `owner`, into `out`: the count written. Runs in the caller's task, never inside the owner.
    pub unsafe fn describe(&self, core: *const c_void, owner: u8, out: &mut [u32]) -> usize {
        match self.describe.as_ref() {
            Some(d) => (d.describe)(core, owner, out.as_mut_ptr(), out.len() as u32) as usize,
            None => 0,
        }
    }

    // ---- the state part

    /// The `(version, accepts_from)` of the state part; None without it.
    pub fn state_versions(&self) -> Option<(u16, u16)> {
        // SAFETY: a non-null table was checked inside the package by `resolve`.
        unsafe { self.state.as_ref() }.map(|t| (t.version, t.accepts_from))
    }
    /// Can this copy take bytes of `old` layout: `accepts_from <= old <= version`. False without the part.
    pub fn state_accepts(&self, old: u16) -> bool {
        self.state_versions()
            .is_some_and(|(version, from)| from <= old && old <= version)
    }
    /// The most bytes `state_export` writes: 0 without the part (or when the owner is busy).
    pub unsafe fn state_export_max(&self, core: *const c_void) -> usize {
        match self.state.as_ref() {
            Some(t) => (t.export_max)(self.instance, core) as usize,
            None => 0,
        }
    }
    /// Export into `out`: the count written (at most `out.len()`); 0 without the part.
    pub unsafe fn state_export(&self, core: *const c_void, out: &mut [u8]) -> usize {
        match self.state.as_ref() {
            Some(t) => ((t.export)(self.instance, core, out.as_mut_ptr(), out.len() as u32)
                as usize)
                .min(out.len()),
            None => 0,
        }
    }
    /// Import into this copy. `from` is the old copy's `(version, bytes)`; it reaches the service only when `state_accepts(version)`, else the service gets `None`.
    /// OK without the part.
    pub unsafe fn state_import(&self, core: *const c_void, from: Option<(u16, &[u8])>) -> i32 {
        let Some(t) = self.state.as_ref() else {
            return OK;
        };
        match from.filter(|(v, _)| self.state_accepts(*v)) {
            Some((_, b)) => (t.import)(self.instance, core, b.as_ptr(), b.len() as u32, true),
            None => (t.import)(self.instance, core, core::ptr::null(), 0, false),
        }
    }
}

#[cfg(test)]
mod tests;
