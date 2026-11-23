//! `shortcuts` service, the `Records` part (interface B): the SHORTCUT records of the `register` section become bindings in the owner `Shortcuts`. `begin` and `commit` are the
//! builder call's side (`builder::Builder`), `check` the validation over the block the services wrote, `apply` one record of a consumer that came up. The free [`validate`] is the
//! rule for the records without a core; `patchkit records --check` calls it too. A consumer that goes down is `Provider::detach` (`shortcuts.rs`).
use crate::bindings::{Added, Binding, Callback, ID_BYTES, LABEL_BYTES, MAX_BINDINGS};
use crate::rule::{at, BODY_LEN, MAX_PER_MODULE};
use crate::shortcuts::Shortcuts;
use core::ffi::c_void;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_abi::platform::DeviceProfile;
use dkrt_abi::provider::{Consumer, Records, Status};
use dkrt_abi::registry::each;
use dkrt_abi::service::{check_step, CheckCtx, ModuleCommitCtx, Problem};
use dkrt_sdk::records::KIND_SHORTCUT;
use dkrt_sdk::*;

/// The warn level flag of a log record.
const LOG_WARN: u16 = logk::L_WARN << logk::LEVEL_SHIFT;

/// The rule for the shortcut records of one module (`rule::check_set`); a kind other than SHORTCUT is accepted. `Records::validate` is this function, and a host tool
/// that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if kind == KIND_SHORTCUT {
        crate::rule::check_set(bodies, report);
    }
}

/// `N` bytes of the record body at `off`.
///
/// # Safety
/// `body` holds at least `off + N` bytes.
unsafe fn text<const N: usize>(body: *const u8, off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = *body.add(off + i);
        i += 1;
    }
    out
}

/// A callback address of the module at `base`: the record holds an offset, 0 for none (offset 0 is the module header, never code).
fn callback(base: u32, offset: u32) -> Option<Callback> {
    if offset == 0 {
        None
    } else {
        // SAFETY: the offset was checked against the image size at the commit; the address is code of the loaded module
        Some(unsafe { core::mem::transmute::<usize, Callback>(base.wrapping_add(offset) as usize) })
    }
}

impl<C: CoreApi> Records<C> for Shortcuts {
    fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    fn begin(&mut self, _core: &C, module: usize) {
        self.builder().begin(module)
    }

    fn commit(&mut self, _core: &C, module: usize, c: &ModuleCommitCtx) -> Result<(), Status> {
        // SAFETY: `c.sink` is the loader's sink.
        unsafe { self.builder().commit(module, c) }
    }

    fn check(&mut self, core: &C, step: u32, c: &CheckCtx) -> Result<(), Status> {
        if step == check_step::RECORDS {
            // the records meet the rule of `rule::check_set` (this also runs at load for a module built by hand), and the callbacks are inside the image
            let mut bodies: [&[u8]; MAX_PER_MODULE + 1] = [&[]; MAX_PER_MODULE + 1];
            let mut n = 0;
            let mut r = OK;
            // SAFETY: `c.reg` is the section the registry walker checked.
            unsafe {
                each(c.reg, |kind, b| {
                    if kind == KIND_SHORTCUT {
                        if n <= MAX_PER_MODULE {
                            bodies[n] = core::slice::from_raw_parts(b, BODY_LEN);
                        }
                        n += 1;
                        if rd32(b.add(at::ON)) >= c.image_size
                            || rd32(b.add(at::OFF)) >= c.image_size
                        {
                            r = E_FORMAT;
                        }
                    }
                });
            }
            let shown = if n < bodies.len() { n } else { bodies.len() };
            let mut first = OK;
            crate::rule::check_set(&bodies[..shown], &mut |p: Problem| {
                if first == OK {
                    first = p.code;
                }
            });
            let refusal = if first != OK { first } else { r };
            if refusal == OK {
                Ok(())
            } else {
                Err(refusal)
            }
        } else if step == check_step::ROWS {
            // room in the table: what the module being reloaded holds is given back before its new file loads
            let mut add = 0;
            // SAFETY: as above.
            unsafe {
                each(c.reg, |kind, _| {
                    if kind == KIND_SHORTCUT {
                        add += 1;
                    }
                });
            }
            let t = self.table();
            if t.len() - t.count_of(core.live_op() as u8) + add > MAX_BINDINGS {
                Err(E_TABLE_FULL)
            } else {
                Ok(())
            }
        } else {
            Ok(())
        }
    }

    fn apply(
        &mut self,
        core: &C,
        kind: u32,
        body: &[u8],
        consumer: &Consumer<'_>,
    ) -> Result<(), Status> {
        if kind != KIND_SHORTCUT {
            return Ok(());
        }
        if body.len() < BODY_LEN {
            return Err(E_FORMAT);
        }
        let owner = consumer.id().get();
        let base = consumer.base();
        // SAFETY: the offsets of `at` are inside `BODY_LEN`, which `body` holds.
        let word = |off: usize| unsafe { rd32(body.as_ptr().add(off)) };
        let keys = [word(at::KEY0) as u8, word(at::KEY1) as u8];
        // a key the device lacks (no panel code): the binding can never complete, and it must not keep the pair from a later binding that can
        let inactive = match DeviceProfile::by_id(core.device_id()) {
            Some(p) => p.key_code(u32::from(keys[0])) == 0 || p.key_code(u32::from(keys[1])) == 0,
            None => true,
        };
        let b = Binding {
            owner,
            keys,
            disabled: false,
            inactive,
            on: callback(base, word(at::ON)),
            off: callback(base, word(at::OFF)),
            arg: word(at::ARG) as usize as *mut c_void,
            // SAFETY: `body` holds `BODY_LEN` bytes (checked above); `at::ID` and `at::LABEL` end inside it.
            id: unsafe { text::<ID_BYTES>(body.as_ptr(), at::ID) },
            label: unsafe { text::<LABEL_BYTES>(body.as_ptr(), at::LABEL) },
        };
        match self.table_mut().add(b) {
            Added::Enabled => Ok(()),
            Added::Disabled { owner: keeper } => {
                // not an error: the module loads, its shortcut does nothing
                // SAFETY: a plain log record of the core.
                unsafe {
                    core.log(
                        logk::SHORTCUT_CLASH,
                        LOG_WARN,
                        [
                            u32::from(owner),
                            u32::from(keeper),
                            u32::from(keys[0]),
                            u32::from(keys[1]),
                            0,
                        ],
                    )
                };
                Ok(())
            }
            Added::Inactive => {
                // SAFETY: as above.
                unsafe {
                    core.log(
                        logk::SHORTCUT_INACTIVE,
                        LOG_WARN,
                        [
                            u32::from(owner),
                            u32::from(keys[0]),
                            u32::from(keys[1]),
                            0,
                            0,
                        ],
                    )
                };
                Ok(())
            }
            Added::Full => Err(E_TABLE_FULL),
        }
    }
}
