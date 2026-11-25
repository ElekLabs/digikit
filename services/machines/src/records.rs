//! `machines` service, the `Records` part (interface B) of the owner `Machines`: the machine, parameter, page, click-action and native-page records of the `register` section,
//! and the free [`validate`] that `patchkit records --check` calls. `begin` and `commit` are the builder calls' side (`build::Builder`), `check` the seven steps over the block the
//! services wrote, `apply` one native page record of a consumer that came up (the rows came up in `Provider::attach`, `machines.rs`). A consumer that goes down is
//! `Provider::detach`.
use crate::check::{
    ck_actions, ck_counts, ck_device, ck_native, ck_owners, ck_records, ck_rows, counts_of, fits,
    Ctx,
};
use crate::machines::{status, Machines};
use crate::native_page::record;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{Consumer, Records, Status};
use dkrt_abi::service::{check_step, CheckCtx, ModuleCommitCtx, Problem};
use dkrt_sdk::*;

pub use crate::modules::{module_info, publish, types_of};
pub use crate::registry::{clear_mach_owners, mach_owner, param_first, set_mach_owner};

/// The rule for the parameter, machine, page and action records of one module (`validate::Acc`); a native page kind and a kind of another service are accepted.
/// `Records::validate` is this function, and a host tool that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    crate::validate::records(kind, bodies, report)
}

impl<C: CoreApi> Records<C> for Machines {
    fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    fn begin(&mut self, _core: &C, module: usize) {
        self.builder().begin(module)
    }

    fn commit(&mut self, core: &C, module: usize, c: &ModuleCommitCtx) -> Result<(), Status> {
        // SAFETY: `c.sink` and `c.keys` are the loader's
        status(unsafe { self.commit_builder(core, module, c) })
    }

    fn check(&mut self, core: &C, step: u32, c: &CheckCtx) -> Result<(), Status> {
        use check_step as st;
        // SAFETY: `c.reg` is the section the registry walker checked; `inf` is the registry's table
        unsafe {
            let inf = dkrt_svc_rows::rows_info(core);
            if inf.is_null() || !fits(inf) {
                // the machine, parameter, page, action and native page records need this registry: a section without them does not
                let none = counts_of(c.reg) == (0, 0, 0, 0, 0);
                return if step == st::COUNTS && !none {
                    Err(E_NO_REGISTRY)
                } else {
                    Ok(())
                };
            }
            let c = &Ctx {
                reg: c.reg,
                image_size: c.image_size,
                inf,
                state: self.reg(),
            };
            status(if step == st::COUNTS {
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
            })
        }
    }

    /// A native page record, in file order after every machine row is in: its callbacks are registered. Other kinds: nothing to do.
    fn apply(
        &mut self,
        core: &C,
        kind: u32,
        body: &[u8],
        consumer: &Consumer<'_>,
    ) -> Result<(), Status> {
        if !record::is_kind(kind) {
            return Ok(());
        }
        let whole = if kind == record::KIND_V2 {
            record::LEN_V2
        } else {
            record::LEN
        };
        if body.len() < whole as usize - 8 {
            return Err(E_FORMAT);
        }
        // SAFETY: `body` holds the record's body; the registry and the page functions are the image's, through the core
        status(unsafe { self.apply_native_page(core, kind, body.as_ptr(), consumer) })
    }
}
