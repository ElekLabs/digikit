//! `fx` service, record handlers (interface B): the effect records of the `register` section, and the effect parts of the DSP
//! section. The registry walker (`registry.rs`) and the DSP loader (`dsp/mod.rs`) call them through `handlers::Service`.
use crate::{fxpage, fxsend};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job, MAX_FX};
use dkrt_abi::mem::rd32;
use dkrt_abi::registry::each;
use dkrt_abi::service::{
    tick_phase, ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, Problem, RowWrite, Service,
    TableFault,
};
use dkrt_sdk::*;

/// The fx service; every method works over the core it is given.
pub struct Fx;

impl Fx {
    /// The rule for the effect records of one module (`validate::effects`); a kind other than the effect record is accepted. `Service::validate_records` is this function,
    /// and a host tool that has no core (`patchkit records --check`) calls it directly.
    pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        crate::validate::effects(kind, bodies, report)
    }
}

impl<C: CoreApi> Service<C> for Fx {
    fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        Fx::validate(kind, bodies, report)
    }

    // An effect module is torn down like any other: the loader's `dsp_clear` (the row's pc to "free" first, so the chain skips it at once: the master chain passes the
    // signal dry, a send bus returns nothing) and `remove` (page, list, send cell, defaults) are the teardown, whatever route the effect runs on. The route and the send
    // amounts are settings keyed by the module id, so the next load of the module gets them back; the knob values stay in the kit's array (`fxpage::apply`).
    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(fxpage::KIND_EFFECT, fxpage::LEN_EFFECT);
    }

    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        use dkrt_abi::service::check_step as st;
        if step == st::COUNTS {
            // the most effects the chain takes
            let mut ne = 0u32;
            each(c.reg, |kind, _| {
                if kind == fxpage::KIND_EFFECT {
                    ne += 1;
                }
            });
            if ne as usize > MAX_FX {
                return E_FORMAT;
            }
            OK
        } else if step == st::EFFECTS {
            // effect pages need core/fxpages; the records meet the rule of `validate` (each record, and an effect has one page)
            let mut bodies: [&[u8]; MAX_FX + 1] = [&[]; MAX_FX + 1];
            let mut n = 0;
            each(c.reg, |kind, b| {
                if kind == fxpage::KIND_EFFECT {
                    if n <= MAX_FX {
                        bodies[n] = core::slice::from_raw_parts(b, crate::validate::BODY_LEN);
                    }
                    n += 1;
                }
            });
            if n == 0 {
                return OK;
            }
            if fxpage::info(core).is_null() {
                return E_NO_REGISTRY;
            }
            let mut first = OK;
            let shown = if n < bodies.len() { n } else { bodies.len() };
            crate::validate::effects(fxpage::KIND_EFFECT, &bodies[..shown], &mut |p: Problem| {
                if first == OK {
                    first = p.code;
                }
            });
            first
        } else {
            OK
        }
    }

    /// An effect record, in file order: its page, parameter rows and list entry are written (the effect's row in the chain is claimed).
    unsafe fn apply_record(core: &C, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        if kind == fxpage::KIND_EFFECT {
            fxpage::apply(core, c.owner, body)
        } else {
            OK
        }
    }

    unsafe fn remove(core: &C, owner: u8) {
        crate::build::release(owner as usize - 1);
        fxpage::remove(core, owner);
    }

    unsafe fn module_begin(_core: &C, rec: usize) {
        crate::build::begin(rec)
    }

    unsafe fn module_commit(core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
        crate::build::commit(core, rec, c)
    }

    unsafe fn tick(core: &C, phase: u32) {
        if phase == tick_phase::LATE {
            fxpage::tick(core);
        }
    }

    unsafe fn cap(core: &C, kind: u8) -> Option<(u32, u32)> {
        use dkrt_sdk::proto::*;
        if kind == CAPK_FX_SLOTS {
            Some((crate::dsp::fx_free_rows(core), crate::dsp::fx_used(core)))
        } else if kind == CAPK_SEND_SLOTS {
            Some((fxsend::slots(core), fxsend::used()))
        } else {
            None
        }
    }

    // every site but the voice's names an effect row (the master route, the send route, or a site this runtime does not know), by its byte offset in the table
    unsafe fn attribute(core: &C, site: u32, id: u32) -> Option<usize> {
        crate::dsp::attribute(core, site, id)
    }

    unsafe fn diag(core: &C, words: &[u32; dkrt_sdk::fault::DIAG_WORDS]) {
        crate::dsp::diag(core, words)
    }

    unsafe fn table_check(_core: &C, used: u32, out: &mut TableFault) -> u32 {
        crate::dsp::table_check(used, out)
    }

    unsafe fn dsp_check_sections(core: &C, sec: &DspSec, job: &mut Job) -> i32 {
        crate::dsp::check_sections(core, sec, job)
    }

    unsafe fn dsp_check_rows(core: &C, sec: &DspSec) -> i32 {
        crate::dsp::check_rows(core, sec)
    }

    unsafe fn dsp_claim(core: &C, c: &mut Claim) {
        crate::dsp::claim(core, c)
    }

    unsafe fn dsp_release(core: &C, rec: usize) {
        crate::dsp::release(core, rec)
    }

    unsafe fn dsp_clear(core: &C, rec: usize) -> bool {
        crate::dsp::clear_fx_rows(core, rec)
    }

    unsafe fn dsp_row_step(core: &C, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        crate::dsp::row_step(core, rec, want, write)
    }
}
