//! The `PAYLOADS` items of a file's header table: the bodies a package carries for a service (`Payloads` part, docs/design/digikit-dkm.md). The core checks where a body lies in
//! the file and who owns it; it never reads a body and keeps no table of kinds.
//!
//! Two parts, as the other checks of a file:
//! - `check_payloads` (in `check_body`, so every check of a file runs it): the count, every body inside the file, the owner a hard need of the package. `E_FORMAT` for the first two,
//!   `E_MISSING` for an owner that is no hard need.
//! - `check_payload_owners` (a load's place step, a live check): the owner is loaded and has the part, and says it takes the body (`Payloads::check`). `E_MISSING` for no owner or no part,
//!   the owner's own refusal otherwise. A live check of a file whose owner is not up yet leaves this to the place step (`require_owner` false).
use super::*;
use dkneeds::keys::{Payload, MAX_PAYLOADS};

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// The payload items of the staged file `c`, bounds and owners checked (`check_payloads`), in table order.
    unsafe fn payload_items(c: &Checked) -> Option<Keys<'static>> {
        if c.reg_size == 0 || !in_file(c.reg_off, c.reg_size, c.size) {
            return None;
        }
        Keys::parse(core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize)).ok()
    }

    /// The count, the bounds and the owners of the payloads of the staged file: nothing is written and no service is asked.
    pub(super) unsafe fn check_payloads(&mut self, c: &Checked) -> i32 {
        let Some(keys) = Self::payload_items(c) else {
            return OK;
        };
        if keys.payload_count() > MAX_PAYLOADS {
            return E_FORMAT;
        }
        let mut i = 0;
        while let Some(p) = keys.payload(i) {
            i += 1;
            if !in_file(p.offset, p.bytes, c.size) {
                return E_FORMAT;
            }
            // the owner is a hard need: a package that carries a body for a service cannot load without it
            if !keys.needs().any(|n| n.service == p.service && n.hard) {
                return E_MISSING;
            }
        }
        OK
    }

    /// The loaded service that owns `p`, its row: `None` when no row of that name is loaded.
    fn payload_owner(p: &Payload) -> Option<dkrt_abi::provider::ProviderHandle> {
        crate::package::provider_rows::rows().iter().find(|r| r.name() == p.service).copied()
    }

    /// Every payload of the staged file has an owner that is loaded, adopts the part and takes the body (`Payloads::check`), for consumer `owner` (record + 1). With `require_owner`
    /// false a payload whose owner row is not loaded is left to the place step.
    pub(super) unsafe fn check_payload_owners(&mut self, c: &Checked, owner: u8, require_owner: bool) -> i32 {
        let Some(keys) = Self::payload_items(c) else {
            return OK;
        };
        let core = &crate::package::core_api::REAL_CORE as *const _ as *const core::ffi::c_void;
        let mut i = 0;
        while let Some(p) = keys.payload(i) {
            i += 1;
            let Some(row) = Self::payload_owner(&p) else {
                if require_owner {
                    return E_MISSING;
                }
                continue;
            };
            if !row.has_payloads() {
                return E_MISSING;
            }
            let body = core::slice::from_raw_parts(c.f.add(p.offset as usize), p.bytes as usize);
            let r = row.payload_check(core, p.kind, body, owner);
            if r != OK {
                return r;
            }
        }
        OK
    }
}
