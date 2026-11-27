//! The registry walker of a module's `register` section: the records the services write at the commit (`Records::commit`) are checked and applied by the services that own
//! their kinds. The walker checks the layout against the lengths the services declare (kind and length of each record, the section's header and device) and calls each service in
//! table order: `check` once per step over all services (the order of the steps is the order of the old single body), `apply`, then `apply_record` for each record in file order,
//! and `remove` in reverse order. It names no service and no table of an adapter; what a service needs of the image it finds itself (`CoreApi::info`). Everything here runs in the
//! UI task. The loader calls it (`apply` from the completion step of the boot-time load, `remove` from the undo); it never calls the loader.
use crate::package::handlers::{check_step, services, ApplyCtx, CheckCtx, ServiceTable};
use dkrt_sdk::*;

pub(crate) use dkrt_abi::registry::each;

/// Validate the `register` records of a module file. `reg` points into the file (the staging buffer). `device` is the device id the section must name. Nothing is written.
pub unsafe fn check(reg: *const u8, size: u32, image_size: u32, device: u32) -> i32 {
    check_with(services(), reg, size, image_size, device)
}

/// The walker of `check`: the layout of the section against the lengths the services declare, then each service's own records.
pub(crate) unsafe fn check_with(l: ServiceTable, reg: *const u8, size: u32, image_size: u32, device: u32) -> i32 {
    if size == 0 {
        return OK;
    }
    // the layout: every record has a kind some service declared, and that kind's length
    let r = dkrt_abi::regcheck::check_layout(reg, size, device, |kind| l.len(kind));
    if r != OK {
        return r;
    }
    // the steps of the old single body, each over every service's records in file order
    let c = CheckCtx { reg, image_size };
    let mut step = 0;
    while step < check_step::COUNT {
        let r = l.check(step, &c);
        if r != OK {
            return r;
        }
        step += 1;
    }
    OK
}

/// Fill the reserved rows from the (checked) records. `owner` = module record index + 1.
pub unsafe fn apply(owner: u8, reg: *const u8, size: u32, base: u32) -> i32 {
    apply_with(services(), owner, reg, size, base)
}

/// The walker of `apply`: each service writes its records, in list order; the first failure stops the walk.
pub(crate) unsafe fn apply_with(l: ServiceTable, owner: u8, reg: *const u8, size: u32, base: u32) -> i32 {
    if size == 0 {
        return OK;
    }
    let c = ApplyCtx { owner, reg, base };
    let mut rc = l.apply(&c);
    if rc == OK {
        // the records that register callbacks and pages, in file order; the first failure stops the walk
        each(reg, |kind, b| {
            if rc == OK {
                rc = l.apply_record(kind, b, &c);
            }
        });
    }
    rc
}

/// Give back every row `owner` filled: each service gives back its own, the last service of the list first.
pub unsafe fn remove(owner: u8) {
    remove_with(services(), owner)
}

pub(crate) unsafe fn remove_with(l: ServiceTable, owner: u8) {
    l.remove(owner);
}
