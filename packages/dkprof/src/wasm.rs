//! The C ABI of `dkprof.wasm` (built by `tools/nx/wasm.mjs` into `apps/web/public/dkprof.wasm`). No bindgen, no firmware inside.
//!
//! Every call takes JSON in linear memory (`dp_alloc` a buffer, write the request, call, `dp_free`) and returns 0 with the answer in the output buffer
//! (`dp_out_ptr`/`dp_out_len`), or 1 with `{"error": "..."}` there. A request with `"format": "text"` gets `{"text": "..."}` (the view of the
//! JSON answer) instead of the data.
//!
//! | call | request | answer |
//! | --- | --- | --- |
//! | `dp_addr_lookup` | `{maps, loads?, snapshot?, dsp_base?, ret?, addrs: [{cpu, addr}]}` | `{unloaded, results}` ([`crate::lookup`]) |
//! | `dp_load_table` | `{maps, snapshot?}` | the load table, or `{reads}` without a snapshot |
//! | `dp_profile_report` | `{profile, maps?, loads?, analyses?, top?}` (`analyses`: module id to the `dksharc::Analysis` of its DSP code) | [`crate::report::Report`] |
//! | `dp_calibrate_fit` | `{device: [profile], model: [profile], block_cycles?, model_id?, sources?}` | `dk.calibration/1` ([`crate::calibrate`]), or `{text}` |
//! | `dp_calibrate_report` | `{calibration, profile?}` | `{text}`: the calibration and, with a model `profile`, the estimated device cycles of its DSP rows (`estimate` in JSON form with `format: "json"`) |
//! | `dp_module_analyse` | the `.dkm` bytes and the `.dko.json` bytes as two buffers, and a text flag | `dksharc::Analysis`, or the text view |

use crate::lookup::{self, LoadTableReq, LookupReq};
use crate::profile::Profile;
use crate::{calibrate, module, report};
use dkaddrmap::{AddrMap, LoadTable};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn finish(r: Result<String, String>) -> i32 {
    OUT.with(|o| match r {
        Ok(s) => {
            *o.borrow_mut() = s.into_bytes();
            0
        }
        Err(e) => {
            *o.borrow_mut() = json!({ "error": e }).to_string().into_bytes();
            1
        }
    })
}

fn text_of(t: String) -> String {
    json!({ "text": t }).to_string()
}

fn wants_text(req: &Value) -> bool {
    req["format"] == "text"
}

fn parse<T: for<'a> Deserialize<'a>>(req: &[u8], what: &str) -> Result<(T, Value), String> {
    let v: Value = serde_json::from_slice(req).map_err(|e| format!("{what}: {e}"))?;
    let t = serde_json::from_value(v.clone()).map_err(|e| format!("{what}: {e}"))?;
    Ok((t, v))
}

pub fn addr_lookup(req: &[u8]) -> Result<String, String> {
    let (r, v): (LookupReq, Value) = parse(req, "addr_lookup")?;
    let a = lookup::addr_lookup(&r)?;
    Ok(if wants_text(&v) {
        text_of(lookup::lookup_text(&a))
    } else {
        a.to_string()
    })
}

pub fn load_table(req: &[u8]) -> Result<String, String> {
    let (r, _): (LoadTableReq, Value) = parse(req, "load_table")?;
    Ok(lookup::load_table(&r)?.to_string())
}

#[derive(Deserialize)]
struct ReportReq {
    profile: Value,
    #[serde(default)]
    maps: Vec<AddrMap>,
    #[serde(default)]
    loads: LoadTable,
    /// Module id to the static analysis of its DSP code: SHARC counters are also added per loop.
    #[serde(default)]
    analyses: BTreeMap<String, dksharc::Analysis>,
    #[serde(default = "default_top")]
    top: usize,
}

fn default_top() -> usize {
    30
}

pub fn profile_report(req: &[u8]) -> Result<String, String> {
    let (r, v): (ReportReq, Value) = parse(req, "profile_report")?;
    let p = Profile::parse(&r.profile.to_string())?;
    let rep = report::build_with(&p, &r.maps, &r.loads, &r.analyses, r.top)?;
    Ok(if wants_text(&v) {
        text_of(report::text(&rep, r.top))
    } else {
        serde_json::to_string(&rep).map_err(|e| e.to_string())?
    })
}

pub fn calibrate_fit(req: &[u8]) -> Result<String, String> {
    let (r, v): (calibrate::Request, Value) = parse(req, "calibrate_fit")?;
    let c = calibrate::fit(&r)?;
    Ok(if wants_text(&v) {
        text_of(calibrate::text(&c))
    } else {
        serde_json::to_string(&c).map_err(|e| e.to_string())?
    })
}

#[derive(Deserialize)]
struct CalReportReq {
    calibration: calibrate::Calibration,
    #[serde(default)]
    profile: Option<Value>,
    /// With a profile: only the estimate in the text, not the calibration's own report.
    #[serde(default)]
    estimate_only: bool,
}

pub fn calibrate_report(req: &[u8]) -> Result<String, String> {
    let (r, v): (CalReportReq, Value) = parse(req, "calibrate_report")?;
    let mut est = None;
    let mut text = if r.estimate_only && r.profile.is_some() {
        String::new()
    } else {
        calibrate::text(&r.calibration)
    };
    if let Some(p) = &r.profile {
        let (j, t) = calibrate::estimate(&r.calibration, p)?;
        text.push_str(&t);
        est = Some(j);
    }
    Ok(if wants_text(&v) {
        text_of(text)
    } else {
        json!({ "calibration": r.calibration, "estimate": est.map(|e| e["estimate"].clone()) })
            .to_string()
    })
}

pub fn module_analyse(dkm: &[u8], dko: &[u8], text: bool) -> Result<String, String> {
    let a = module::analyse(dkm, dko)?;
    if text {
        Ok(text_of(module::text(&a)))
    } else {
        serde_json::to_string(&a).map_err(|e| e.to_string())
    }
}

/// # Safety
/// `p..p+n` must be a readable range.
unsafe fn slice<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(p, n) }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dp_alloc(n: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(n.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `p` and `n` must be from `dp_alloc(n)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_free(p: *mut u8, n: usize) {
    drop(unsafe { Vec::from_raw_parts(p, 0, n.max(1)) });
}

/// # Safety
/// `req..req+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_addr_lookup(req: *const u8, len: usize) -> i32 {
    finish(addr_lookup(unsafe { slice(req, len) }))
}

/// # Safety
/// `req..req+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_load_table(req: *const u8, len: usize) -> i32 {
    finish(load_table(unsafe { slice(req, len) }))
}

/// # Safety
/// `req..req+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_profile_report(req: *const u8, len: usize) -> i32 {
    finish(profile_report(unsafe { slice(req, len) }))
}

/// # Safety
/// `req..req+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_calibrate_fit(req: *const u8, len: usize) -> i32 {
    finish(calibrate_fit(unsafe { slice(req, len) }))
}

/// # Safety
/// `req..req+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_calibrate_report(req: *const u8, len: usize) -> i32 {
    finish(calibrate_report(unsafe { slice(req, len) }))
}

/// # Safety
/// Both ranges must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dp_module_analyse(
    dkm: *const u8,
    dkm_len: usize,
    dko: *const u8,
    dko_len: usize,
    text: i32,
) -> i32 {
    finish(module_analyse(
        unsafe { slice(dkm, dkm_len) },
        unsafe { slice(dko, dko_len) },
        text != 0,
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn dp_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn dp_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out() -> Value {
        let n = dp_out_len();
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(dp_out_ptr(), n) }).unwrap()
    }

    #[test]
    fn the_c_abi_reports_errors_as_json_and_answers_as_text_on_request() {
        let bad = b"{";
        assert_eq!(unsafe { dp_addr_lookup(bad.as_ptr(), bad.len()) }, 1);
        assert!(out()["error"].as_str().unwrap().contains("addr_lookup"));
        let req = br#"{"maps":[],"addrs":[{"cpu":"cf","addr":16}],"format":"text"}"#;
        assert_eq!(unsafe { dp_addr_lookup(req.as_ptr(), req.len()) }, 0);
        assert_eq!(out()["text"], "0x10 ?\n");
    }
}
