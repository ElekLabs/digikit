//! `shortcuts` service, handlers (interface B): the SHORTCUT records of the `register` section become bindings in the service's `Shortcuts`; `remove` drops a module's bindings.
//! The builder call that makes the records is `build::shortcut_add`.
//!
//! The service keeps one `Shortcuts` (a zero-filled static). `on_key` is the entry for the key-dispatch hook, which is not installed yet: until it is, nothing calls it.
use crate::bindings::{Added, Binding, Callback, ID_BYTES, LABEL_BYTES, MAX_BINDINGS};
use crate::shortcuts::{Shortcuts, Verdict};
use crate::validate::{at, BODY_LEN, MAX_PER_MODULE};
use core::ffi::c_void;
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_abi::platform::DeviceProfile;
use dkrt_abi::registry::each;
use dkrt_abi::service::{
    check_step, tick_phase, ApplyCtx, CheckCtx, Declare, ModuleCommitCtx, Problem, Service,
};
use dkrt_sdk::records::{KIND_SHORTCUT, LEN_SHORTCUT};
use dkrt_sdk::*;

/// The warn level flag of a log record.
const LOG_WARN: u16 = logk::L_WARN << logk::LEVEL_SHIFT;

/// The state: no key held, no binding. All zero bytes, so it is `.bss`.
///
/// One task owns it: the UI task. The key hook (the firmware's key walker) and `Service::tick` run in it, and so do the loads, unloads and reloads of modules (`Service::remove`,
/// `apply_record`): a SysEx command only queues a live job, whose steps run once per UI pass (`reload::poll` in `dkrt_tick`), and the boot's loads and a failed load's removal run
/// in the UI pass as well. So no lock is needed and `on_key`, `run_pending` and `remove` never overlap.
static mut STATE: Shortcuts = unsafe { core::mem::zeroed() };

/// The service's one `Shortcuts` (host tests read and drive it).
pub fn state() -> &'static mut Shortcuts {
    unsafe { &mut *addr_of_mut!(STATE) }
}

/// One key event of the panel for the dispatch hook: `code` is the key's id in the event, `flags` the event's flags word (read with `[input.key_flags]`). It decides and queues the module's calls; it runs no module
/// code (the UI pass does, `Service::tick`).
pub fn on_key<C: CoreApi>(core: &C, code: u32, flags: u32) -> Verdict {
    match DeviceProfile::by_id(core.device_id()) {
        Some(p) => state().on_key(&p, code, crate::shortcuts::classify_flags(&p, flags)),
        None => Verdict::Pass,
    }
}

/// The shortcuts service.
pub struct ShortcutsService;

/// The rule for the shortcut records of one module (`validate::check_set`); a kind other than SHORTCUT is accepted. The service's `validate_records` is this function, and a host tool
/// that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if kind == KIND_SHORTCUT {
        crate::validate::check_set(bodies, report);
    }
}

fn text<const N: usize>(body: *const u8, off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = unsafe { *body.add(off + i) };
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

impl<C: CoreApi> Service<C> for ShortcutsService {
    fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(KIND_SHORTCUT, LEN_SHORTCUT);
    }

    /// The UI pass: make the `on` and `off` calls that the key hook queued, in order.
    unsafe fn tick(_core: &C, phase: u32) {
        if phase == tick_phase::EARLY {
            state().run_pending();
        }
    }

    unsafe fn module_begin(_core: &C, rec: usize) {
        crate::build::begin(rec)
    }

    unsafe fn module_commit(core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
        crate::build::commit(core, rec, c)
    }

    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        if step == check_step::RECORDS {
            // the records meet the rule of `validate` (this also runs at load for a module built by hand), and the callbacks are inside the image
            let mut bodies: [&[u8]; MAX_PER_MODULE + 1] = [&[]; MAX_PER_MODULE + 1];
            let mut n = 0;
            let mut r = OK;
            each(c.reg, |kind, b| {
                if kind == KIND_SHORTCUT {
                    if n <= MAX_PER_MODULE {
                        bodies[n] = core::slice::from_raw_parts(b, BODY_LEN);
                    }
                    n += 1;
                    if rd32(b.add(at::ON)) >= c.image_size || rd32(b.add(at::OFF)) >= c.image_size {
                        r = E_FORMAT;
                    }
                }
            });
            let shown = if n < bodies.len() { n } else { bodies.len() };
            let mut first = OK;
            crate::validate::check_set(&bodies[..shown], &mut |p: Problem| {
                if first == OK {
                    first = p.code;
                }
            });
            if first != OK {
                first
            } else {
                r
            }
        } else if step == check_step::ROWS {
            // room in the table: what the module being reloaded holds is given back before its new file loads
            let mut add = 0;
            each(c.reg, |kind, _| {
                if kind == KIND_SHORTCUT {
                    add += 1;
                }
            });
            let t = state().table();
            if t.len() - t.count_of(core.live_op() as u8) + add > MAX_BINDINGS {
                E_TABLE_FULL
            } else {
                OK
            }
        } else {
            OK
        }
    }

    unsafe fn apply_record(core: &C, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        if kind != KIND_SHORTCUT {
            return OK;
        }
        let keys = [
            rd32(body.add(at::KEY0)) as u8,
            rd32(body.add(at::KEY1)) as u8,
        ];
        // a key the device lacks (no panel code): the binding can never complete, and it must not keep the pair from a later binding that can
        let inactive = match DeviceProfile::by_id(core.device_id()) {
            Some(p) => p.key_code(u32::from(keys[0])) == 0 || p.key_code(u32::from(keys[1])) == 0,
            None => true,
        };
        let b = Binding {
            owner: c.owner,
            keys,
            disabled: false,
            inactive,
            on: callback(c.base, rd32(body.add(at::ON))),
            off: callback(c.base, rd32(body.add(at::OFF))),
            arg: rd32(body.add(at::ARG)) as usize as *mut c_void,
            id: text::<ID_BYTES>(body, at::ID),
            label: text::<LABEL_BYTES>(body, at::LABEL),
        };
        match state().table_mut().add(b) {
            Added::Enabled => OK,
            Added::Disabled { owner } => {
                // not an error: the module loads, its shortcut does nothing
                core.log(
                    logk::SHORTCUT_CLASH,
                    LOG_WARN,
                    [
                        u32::from(c.owner),
                        u32::from(owner),
                        u32::from(keys[0]),
                        u32::from(keys[1]),
                        0,
                    ],
                );
                OK
            }
            Added::Inactive => {
                core.log(
                    logk::SHORTCUT_INACTIVE,
                    LOG_WARN,
                    [
                        u32::from(c.owner),
                        u32::from(keys[0]),
                        u32::from(keys[1]),
                        0,
                        0,
                    ],
                );
                OK
            }
            Added::Full => E_TABLE_FULL,
        }
    }

    unsafe fn remove(_core: &C, owner: u8) {
        crate::build::end();
        state().remove(owner);
    }
}
