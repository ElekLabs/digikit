//! The route of an effect: record flags, saved setting and the describe words.

use super::*;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;
use dkrt_abi::provider::WordSink;
use dkrt_sdk::*;

/// The routes of a record's flags word: (allowed `ROUTE_*` bits, default `ROUTE_*`). The loader has checked the word.
pub(super) fn routes_of(flags: u32) -> (u8, u8) {
    let mut allow = 0;
    if flags & FXF_NOMASTER == 0 {
        allow |= ROUTE_MASTER;
    }
    if flags & FXF_SEND != 0 {
        allow |= ROUTE_SEND;
    }
    (
        allow,
        if flags & FXF_DEFAULT_SEND != 0 {
            ROUTE_SEND
        } else {
            ROUTE_MASTER
        },
    )
}

/// The route effect row `k` has in the settings, as `ROUTE_*`: the stored one when the effect allows it, else its default. UI task.
pub(super) unsafe fn saved_route<C: CoreApi>(core: &C, k: usize) -> u8 {
    let s = ss();
    let mut key = [0u8; KEY_MAX];
    let kn = key_of(&mut key, b"fx/route/", &s.id[k], s.idlen[k] as usize);
    let mut v = [0u8; 4];
    if core.settings_get(&key[..kn], &mut v) == 1 {
        let r = if v[0] == 1 {
            ROUTE_SEND
        } else if v[0] == 0 {
            ROUTE_MASTER
        } else {
            0
        };
        if r != 0 && s.allow[k] & r != 0 {
            return r;
        }
    }
    s.dflt[k]
}

/// The words of the MODULES reply for module record `owner` (1 ..): one for each effect row of the module that has a page, in row order (the `Describe` of the service). Bits 0 to 7
/// the row, then four bits each: the route it runs on, the route the settings give it now (differs from the running one after a pick that waits for its reload), the routes its
/// record allows and its default (`ROUTE_*`). It runs on the MODULES command handler's task, outside the owner: it reads the tables of the service by name and one settings value;
/// each is a byte or a word that a task switch cannot tear.
pub unsafe fn describe<C: CoreApi>(core: &C, owner: u8, out: &mut WordSink) {
    if owner == 0 {
        return;
    }
    let s = ss();
    let mut k = 0;
    while k < dsp::MAX_FX {
        if fxpage::page_owner(k) == owner {
            let running = if s.route[k] != 0 {
                ROUTE_SEND
            } else {
                ROUTE_MASTER
            };
            out.push(
                k as u32
                    | (running as u32) << 8
                    | (saved_route(core, k) as u32) << 12
                    | (s.allow[k] as u32) << 16
                    | (s.dflt[k] as u32) << 20,
            );
        }
        k += 1;
    }
}
