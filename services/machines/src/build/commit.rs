//! The commit (`Records::commit`) of the collection.
use super::*;
use crate::{assign, dsp};
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::iw;
use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
use dkrt_sdk::*;
use dkrt_svc_rows::dynreg::w;

/// The commit of module record `rec` (`Records::commit`): `PREPARE` checks the collection and measures its records, `WRITE` writes them, counts the rows and keeps the callback
/// arguments for `apply`; `DECLARE` (after the registry walker's check) declares the voice rows' ranges. The collection is closed after the `DECLARE` pass.
impl Builder {
    pub unsafe fn commit<C: CoreApi>(
        &mut self,
        core: &C,
        reg: &dkrt_svc_rows::dynreg::RegState,
        rec: usize,
        c: &ModuleCommitCtx,
    ) -> i32 {
        let b = &self.build;
        if !b.gate.is_open_for(rec) {
            return OK; // no `module_begin` for this record: nothing was collected
        }
        if b.err != 0 {
            return b.err;
        }
        if b.is_empty() {
            self.end();
            return OK;
        }
        // a provider adds no machine, parameter or page row of its own
        if rec >= dkrt_abi::MAX_MODULES {
            return E_FORMAT;
        }
        let inf = dkrt_svc_rows::dynreg(core);
        if inf.is_null() || iw(inf, w::VERSION) < 3 {
            return E_NO_REGISTRY;
        }
        let p = Profile::read(inf);
        let mut plan = Plan::new();
        let voice = voices(core, rec, b, c.keys);
        let sink = &mut *c.sink;
        if c.phase == commit_phase::PREPARE {
            let r = check_commit(b, &p, c.keys, &free_rows(core, reg, inf));
            if r != OK {
                return r;
            }
            return match lower(b, &p, c.keys, &voice, c.base, sink, &mut plan) {
                Ok(()) => OK,
                Err(e) => e,
            };
        }
        if c.phase == commit_phase::WRITE {
            if let Err(e) = lower(b, &p, c.keys, &voice, c.base, sink, &mut plan) {
                return e;
            }
            self.args = Some(apply_args(b, &plan));
            let (mr, pr, pg) = counts_of(b, &p);
            sink.rows[0] += mr;
            sink.rows[1] += pr;
            sink.rows[2] += pg;
            return OK;
        }
        // DECLARE: the walker's check found nothing wrong. The plan of the lowering gives each machine's knob positions again (a measuring pass writes nothing).
        if let Err(e) = lower(
            b,
            &p,
            c.keys,
            &voice,
            c.base,
            &mut RecordSink::measure(),
            &mut plan,
        ) {
            return e;
        }
        // the ranges and latch mask of each machine's voice row
        let mut m = 0;
        while m < b.nm {
            let t = assign::resolve(core, machine_key(keys_of(c), &b.machines[m]));
            if let Some(row) = dsp::voice_row(core, rec, t) {
                let (rng, latched) = voice_declared(b, &plan, m);
                if latched != 0 {
                    core.log(
                        logk::MODULE_LATCH,
                        LOG_INFO,
                        [core.boot_index(), m as u32, latched, 0, 0],
                    );
                }
                let r = dsp::set_declared(core, row, rng, latched);
                if r != OK {
                    return r;
                }
            }
            m += 1;
        }
        self.end();
        OK
    }
}

pub(super) fn keys_of<'a>(c: &'a ModuleCommitCtx) -> &'a KeysCopy {
    c.keys
}
