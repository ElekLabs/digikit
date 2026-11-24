//! `fx` service, the `Records` part (interface B) of the owner `Fx`: the effect records of the `register` section, and the free [`validate`] that `patchkit records --check` calls.
//! `begin` and `commit` are the builder calls' side (`builder::Builder`), `check` the validation over the block the services wrote, `apply` one effect record of a consumer that came
//! up (its page, parameter rows and list entry; the effect's row in the chain is claimed). A consumer that goes down is `Provider::detach` (`fx.rs`).
use crate::fx::Fx;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::provider::{Consumer, Records, Status};
use dkrt_abi::registry::each;
use dkrt_abi::service::{check_step, CheckCtx, ModuleCommitCtx, Problem};
use dkrt_sdk::*;

/// The rule for the effect records of one module (`rule::effects`); a kind other than the effect record is accepted. `Records::validate` is this function, and a host tool
/// that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    crate::rule::effects(kind, bodies, report)
}

impl<C: CoreApi> Records<C> for Fx {
    fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    fn begin(&mut self, _core: &C, module: usize) {
        self.builder().begin(module)
    }

    fn commit(&mut self, core: &C, module: usize, c: &ModuleCommitCtx) -> Result<(), Status> {
        // SAFETY: `c.sink` is the loader's sink.
        unsafe { self.builder().commit(core, module, c) }
    }

    fn check(&mut self, core: &C, step: u32, c: &CheckCtx) -> Result<(), Status> {
        if step == check_step::COUNTS {
            // the most effects the chain takes
            let mut ne = 0u32;
            // SAFETY: `c.reg` is the section the registry walker checked.
            unsafe {
                each(c.reg, |kind, _| {
                    if kind == fxpage::KIND_EFFECT {
                        ne += 1;
                    }
                });
            }
            if ne as usize > MAX_FX {
                return Err(E_FORMAT);
            }
            Ok(())
        } else if step == check_step::EFFECTS {
            // effect pages need core/fxpages; the records meet the rule of `validate` (each record, and an effect has one page)
            let mut bodies: [&[u8]; MAX_FX + 1] = [&[]; MAX_FX + 1];
            let mut n = 0;
            // SAFETY: as above; a record of this kind holds `BODY_LEN` bytes after its header.
            unsafe {
                each(c.reg, |kind, b| {
                    if kind == fxpage::KIND_EFFECT {
                        if n <= MAX_FX {
                            bodies[n] = core::slice::from_raw_parts(b, crate::rule::BODY_LEN);
                        }
                        n += 1;
                    }
                });
            }
            if n == 0 {
                return Ok(());
            }
            // SAFETY: reads the image's table through the core
            if unsafe { fxpage::info(core) }.is_null() {
                return Err(E_NO_REGISTRY);
            }
            let mut first = OK;
            let shown = if n < bodies.len() { n } else { bodies.len() };
            crate::rule::effects(fxpage::KIND_EFFECT, &bodies[..shown], &mut |p: Problem| {
                if first == OK {
                    first = p.code;
                }
            });
            if first == OK {
                Ok(())
            } else {
                Err(first)
            }
        } else {
            Ok(())
        }
    }

    /// An effect record, in file order: its page, parameter rows and list entry are written (the effect's row in the chain is claimed).
    fn apply(
        &mut self,
        core: &C,
        kind: u32,
        body: &[u8],
        consumer: &Consumer<'_>,
    ) -> Result<(), Status> {
        if kind != fxpage::KIND_EFFECT {
            return Ok(());
        }
        if body.len() < crate::rule::BODY_LEN {
            return Err(E_FORMAT);
        }
        // SAFETY: `body` holds the `BODY_LEN` bytes of a record; the pages and send cells are this crate's tables
        let r = unsafe { fxpage::apply(core, consumer.id().get(), body.as_ptr()) };
        if r == OK {
            Ok(())
        } else {
            Err(r)
        }
    }
}
