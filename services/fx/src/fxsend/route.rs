//! The route of an effect: record flags, saved setting and the FX ROUTES entry.

use super::*;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;
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

/// Effect `index` (0 ..) in row order, among the rows that have a page: the entry for DIGIKIT > FX ROUTES. 0, or `E_NOT_FOUND` past the end. UI task.
pub unsafe fn fx_entry<C: CoreApi>(core: &C, index: u32, out: &mut DkrtFx) -> i32 {
    let s = ss();
    let mut seen = 0;
    let mut k = 0;
    while k < dsp::MAX_FX {
        if fxpage::page_owner(k) != 0 {
            if seen == index {
                out.row = k as u32;
                out.allowed = s.allow[k] as u32;
                out.dflt = s.dflt[k] as u32;
                out.running = if s.route[k] != 0 {
                    ROUTE_SEND as u32
                } else {
                    ROUTE_MASTER as u32
                };
                out.saved = saved_route(core, k) as u32;
                out.tab = [0; 8];
                fxpage::tab_of(k, &mut out.tab);
                out.id = s.id[k];
                return OK;
            }
            seen += 1;
        }
        k += 1;
    }
    E_NOT_FOUND
}
