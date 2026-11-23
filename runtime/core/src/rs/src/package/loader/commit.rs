//! The commit of a loaded file: its declarations, the services' records and the DSP rows (`Loader::commit`).
use super::*;
use crate::package::provider_rows::RowSource;

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {

    /// The commit of 6.3 steps 1 to 3: the declarations of a service file against its header table, the services' part (they check what their builder calls collected, write their
    /// records and declare their DSP rows), the registry walker over the block; the record block stays in the arena.
    pub(super) unsafe fn commit(&mut self, rec: usize, c: &crate::package::collect::Collect, keys: &crate::package::collect::KeysCopy, image_size: u32) -> i32 {
        use crate::package::collect as col;
        let base = records::module(rec).base;
        // the declarations of a service module against its header table (declare-check-commit)
        if let Some(d) = col::decl_diff(c, keys, base) {
            self.decl_diff = d;
            return E_DECL_KIND;
        }
        // a service file declares its row in `init` (`dk_provider_declare_row`, with the instance); a module declares none. `fail` takes whatever was
        // added down again
        let extent = self.m.image_extent(base, image_size);
        let row = c.decl.provider_row_ptr().map(|(row, instance)| RowSource { row: &*row, instance, extent });
        let mut order = None;
        if let Some(row) = &row {
            if rec < MAX_MODULES {
                return E_DECL_KIND;
            }
            let provider = &self.provs[rec - MAX_MODULES];
            let name = match row.name_hash() {
                Ok(name) => name,
                Err(e) => return e,
            };
            if provider.provides.map(|p| p.0) != Some(name) {
                return E_DECL_KIND;
            }
            order = provider.callback_order;
        }
        let r = self.r.add(rec, row, order);
        if r != OK {
            return r;
        }
        // an error of a generic record (`dk_record_add`); a provider adds no record of its own
        if c.err != 0 {
            return c.err;
        }
        if rec >= MAX_MODULES && c.ng != 0 {
            return E_FORMAT;
        }
        // the services' part of the commit (interface B): they check what their builder calls collected and write their records
        let (baddr, size, rows) = match self.commit_services(rec, c, keys, base, image_size) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if rec < MAX_MODULES {
            col::keep(rec, baddr, size);
        }
        crate::package::resident::dlog::ev(
            logk::MODULE_COMMIT,
            crate::package::resident::dlog::INFO,
            [boot_idx::get(), rows.0 | rows.1 << 8 | rows.2 << 16, baddr, size, 0],
        );
        OK
    }


    /// The commit of the services (`Service::module_commit`): a `PREPARE` pass checks and measures the records, the loader takes one arena block for them, a `WRITE` pass fills it
    /// (the generic records of the module follow the services' records), and the registry walker checks and applies it. Returns the block (address, size; 0, 0 when no record was
    /// written) and the rows the module uses. Nothing stays allocated on an error; the caller's undo gives the services their `remove`.
    unsafe fn commit_services(&mut self, rec: usize, c: &crate::package::collect::Collect, keys: &crate::package::collect::KeysCopy, base: u32, image_size: u32) -> Result<(u32, u32, (u32, u32, u32)), i32> {
        use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
        let mut sink = RecordSink::measure();
        let ctx = ModuleCommitCtx { phase: commit_phase::PREPARE, base, image_size, keys, sink: &mut sink };
        let r = self.r.module_commit(rec, &ctx);
        if r != OK {
            return Err(r);
        }
        c.write_generic(&mut sink);
        let (size, count) = (sink.at, sink.count);
        let blk = if size > RecordSink::HEADER {
            match self.m.alloc(size as u32) {
                Some(b) => Some(b),
                None => return Err(E_NOMEM),
            }
        } else {
            None
        };
        let mut out = match blk {
            Some(b) => RecordSink::over(b.ptr, size),
            None => RecordSink::over(core::ptr::null_mut(), 0),
        };
        let ctx = ModuleCommitCtx { phase: commit_phase::WRITE, base, image_size, keys, sink: &mut out };
        let r = self.r.module_commit(rec, &ctx);
        let free = |l: &mut Self| {
            if let Some(b) = blk {
                l.m.free(b.addr);
            }
        };
        if r != OK {
            free(self);
            return Err(r);
        }
        c.write_generic(&mut out);
        let rows = (out.rows[0], out.rows[1], out.rows[2]);
        if let Some(b) = blk {
            if out.at != size || out.count != count {
                free(self);
                return Err(E_FORMAT); // a service wrote another size than it measured
            }
            wr32(b.ptr, 0x444b_5247);
            wr32(b.ptr.add(4), 1);
            wr32(b.ptr.add(8), self.p.device_id());
            wr32(b.ptr.add(12), count);
            // check before anything of the module's rows is declared: a refusal leaves everything as it was
            let r = crate::package::registry::check(b.ptr, size as u32, image_size, self.p.device_id());
            if r != OK {
                free(self);
                return Err(r);
            }
        }
        let ctx = ModuleCommitCtx { phase: commit_phase::DECLARE, base, image_size, keys, sink: &mut out };
        let r = self.r.module_commit(rec, &ctx);
        if r != OK {
            free(self);
            return Err(r);
        }
        let b = match blk {
            Some(b) => b,
            None => return Ok((0, 0, rows)),
        };
        let r = if self.split_apply && rec < MAX_MODULES {
            // the records are applied when the module is attached, over the rows of its services in one walk (`package_attach`)
            self.reg_block[rec] = (b.ptr as *const u8, size as u32);
            OK
        } else {
            crate::package::registry::apply(rec as u8 + 1, b.ptr, size as u32, base)
        };
        if r != OK {
            free(self);
            return Err(r);
        }
        Ok((b.addr.get(), size as u32, rows))
    }


    /// One pass of the DSP state machine (`dsp::step_with`), with the DSP backend of the installer writing the slots of a service file once its code is acknowledged.
    pub(crate) unsafe fn dsp_advance(&mut self, l: ServiceTable) -> dsp::Advance {
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        let di = &mut self.dsp_install;
        dsp::step_with(l, &mut |rec| di.sequence(rec as u8, area))
    }

    /// Every row of the load in flight is in: the services hear it, the boot record lists the module, the next load starts.
    pub(crate) unsafe fn dsp_loaded(&mut self, l: ServiceTable) {
        let (rec, idx) = {
            let p = &dsp::ds().pending;
            (p.rec as usize, p.boot_idx)
        };
        l.on_loaded(rec);
        crate::package::resident::boot::report_dsp(idx, OK);
        dsp::loaded_done();
    }

    /// The hash of the service that provider `j` provides (its PROVIDES item), 0 when unknown.
    pub(crate) unsafe fn provided_hash(&self, j: usize) -> u32 {
        match Keys::parse(self.found.table(j)) {
            Ok(k) => k.words(dkneeds::keys::tag::PROVIDES, 0).map_or(0, |w| w.0),
            Err(_) => 0,
        }
    }
}
