//! `machines` service, record handlers (interface B): the machine, parameter, page, click-action and native-page records of the
//! `register` section, and the voice parts of the DSP section. The registry walker (`registry.rs`) calls them through
//! `handlers::Service`; the rows and strings they fill are the registry's state (`DKRT_REG`, a static of this service).
use crate::apply::{apply, apply_record, AppCtx};
use crate::check::{
    ck_actions, ck_counts, ck_device, ck_native, ck_owners, ck_records, ck_rows, counts_of, fits,
    Ctx,
};
use crate::parking::{give_way, park_absent, BUSY_PASSES, LOGGED, MISSING_ROWS, PARK_TRIES};
use crate::registry::{counts, MAX_MACH};
use crate::unload::{remove, reset_machine_row};
use crate::view::refresh_view;
use crate::{dsp, slots};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::service::{
    check_step, tick_phase, ApplyCtx, CheckCtx, Claim, Declare, ModuleCommitCtx, Problem, RowWrite,
    Service, SettleCtx, TableFault,
};
use dkrt_sdk::records::{
    KIND_ACTION, KIND_MACHINE, KIND_NATIVE_PAGE, KIND_NATIVE_PAGE_V2, KIND_PAGE, KIND_PARAM,
    LEN_ACTION, LEN_MACHINE, LEN_NATIVE_PAGE, LEN_NATIVE_PAGE_V2, LEN_PAGE, LEN_PARAM,
};
use dkrt_sdk::*;

pub use crate::modules::{module_info, types_of};
pub use crate::registry::{param_first, param_owners, rstate, DKRT_REG};

/// The machines service.
pub struct Machines;

/// The rule for the parameter, machine, page and action records of one module (`validate::Acc`); a native page kind and a kind of another service are accepted.
/// The service's `validate_records` is this function, and a host tool that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    crate::validate::records(kind, bodies, report)
}

impl<C: CoreApi> Service<C> for Machines {
    fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(KIND_PARAM, LEN_PARAM);
        d.kind(KIND_MACHINE, LEN_MACHINE);
        d.kind(KIND_PAGE, LEN_PAGE);
        d.kind(KIND_ACTION, LEN_ACTION);
        d.kind(KIND_NATIVE_PAGE, LEN_NATIVE_PAGE);
        d.kind(KIND_NATIVE_PAGE_V2, LEN_NATIVE_PAGE_V2);
    }

    unsafe fn settle(core: &C, s: &SettleCtx) {
        // a new plan of the numbers: the parking state of the earlier one is not carried over
        PARK_TRIES = 0;
        BUSY_PASSES = 0;
        LOGGED = 0;
        crate::assign::plan(core, s)
    }

    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        use check_step as st;
        let inf = dkrt_svc_params::dynreg(core);
        if inf.is_null() || !fits(inf) {
            // the machine, parameter, page, action and native page records need this registry: a section without them does not
            let none = counts_of(c.reg) == (0, 0, 0, 0, 0);
            return if step == st::COUNTS && !none {
                E_NO_REGISTRY
            } else {
                OK
            };
        }
        let c = &Ctx {
            reg: c.reg,
            image_size: c.image_size,
            inf,
        };
        if step == st::COUNTS {
            ck_counts(c)
        } else if step == st::ACTIONS {
            ck_actions(c)
        } else if step == st::NATIVE {
            ck_native(core, c)
        } else if step == st::DEVICE {
            ck_device(c)
        } else if step == st::RECORDS {
            ck_records(core, c)
        } else if step == st::OWNERS {
            ck_owners(core, c)
        } else if step == st::ROWS {
            ck_rows(core, c)
        } else {
            OK
        }
    }

    unsafe fn apply(core: &C, c: &ApplyCtx) -> i32 {
        let inf = dkrt_svc_params::dynreg(core);
        if inf.is_null() {
            return if counts_of(c.reg) == (0, 0, 0, 0, 0) {
                OK
            } else {
                E_NO_REGISTRY
            };
        }
        apply(
            core,
            &AppCtx {
                owner: c.owner,
                reg: c.reg,
                base: c.base,
                inf,
            },
        )
    }

    unsafe fn apply_record(core: &C, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        apply_record(core, kind, body, c)
    }

    unsafe fn live_done(core: &C) {
        // the placeholder rows whose number the sticky map no longer holds
        dsp::free_placeholders(core);
        // the machine rows of a pinned number whose module did not come back (a MISSING row with no voice row would play the first stock machine's sound): gap rows. A
        // number the map holds stays MISSING (E5d), and `tick` parks its voice row if it has none.
        let inf = dkrt_svc_params::dynreg(core);
        if !inf.is_null() {
            let s = rstate();
            let mut k = 0;
            while k < MAX_MACH {
                if MISSING_ROWS & (1 << k) != 0 && s.mach_owner[k] == 0 {
                    reset_machine_row(core, inf, k, false);
                }
                k += 1;
            }
        }
        MISSING_ROWS = 0;
        PARK_TRIES = 0;
        // the machine selection view caches the list: build it again, after a failed load too (the cached list would keep an entry the rows no longer have)
        refresh_view(core);
    }

    unsafe fn remove(core: &C, owner: u8) {
        crate::build::end();
        remove(core, owner)
    }

    unsafe fn module_begin(_core: &C, rec: usize) {
        crate::build::begin(rec)
    }

    unsafe fn module_commit(core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
        crate::build::commit(core, rec, c)
    }

    unsafe fn tick(core: &C, phase: u32) {
        if phase == tick_phase::LAST {
            slots::tick(core);
            park_absent(core);
        }
    }

    unsafe fn cap(core: &C, kind: u8) -> Option<(u32, u32)> {
        use dkrt_sdk::proto::*;
        // an if chain, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
        if kind == CAPK_MACHINE_ROWS {
            Some(counts(core)[0])
        } else if kind == CAPK_VOICE_ROWS {
            Some(dsp::rows_counts(core))
        } else if kind == CAPK_PARAM_ROWS {
            Some(counts(core)[1])
        } else if kind == CAPK_PAGE_ROWS {
            Some(counts(core)[2])
        } else if kind == CAPK_MAP_ENTRIES {
            Some((dkrt_sdk::sel::MAP_N as u32, crate::assign::map_used()))
        } else {
            None
        }
    }

    unsafe fn attribute(core: &C, site: u32, id: u32) -> Option<usize> {
        if site == dkrt_sdk::fault::SITE_VOICE {
            dsp::module_of_type(core, id)
        } else {
            None
        }
    }

    unsafe fn diag(core: &C, words: &[u32; dkrt_sdk::fault::DIAG_WORDS]) {
        crate::guard::diag(core, words)
    }

    unsafe fn table_check(_core: &C, used: u32, out: &mut TableFault) -> u32 {
        dsp::table_check(used, out)
    }

    unsafe fn dsp_check_sections(core: &C, sec: &DspSec, job: &mut Job) -> i32 {
        dsp::check_sections(core, sec, job)
    }

    unsafe fn dsp_check_rows(core: &C, sec: &DspSec) -> i32 {
        dsp::check_rows(core, sec)
    }

    unsafe fn dsp_check_types(core: &C, sec: &DspSec) -> i32 {
        dsp::check_types(core, sec)
    }

    unsafe fn dsp_admit(core: &C, job: &dkrt_abi::dsp::Job) -> i32 {
        dsp::make_room(core, job, &mut |t| give_way(core, t))
    }

    unsafe fn dsp_claim(core: &C, c: &mut Claim) {
        dsp::claim(core, c)
    }

    unsafe fn dsp_release(core: &C, rec: usize) {
        dsp::release(core, rec)
    }

    unsafe fn dsp_clear(core: &C, rec: usize) -> bool {
        dsp::clear_rows(core, rec)
    }

    unsafe fn dsp_row_step(core: &C, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        dsp::row_step(core, rec, want, write)
    }
}
