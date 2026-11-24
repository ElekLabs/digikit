//! The page list of the FX view.

use super::*;
use crate::fxsend;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::mem::iw;

pub(super) unsafe fn rebuild_list<C: CoreApi>(core: &C, inf: *const u32) {
    let s = fs();
    let list = iw(inf, w::LIST);
    let stock = iw(inf, w::STOCK);
    // the stock count is the byte of `moveq N,D1` in the view builder
    let n_stock = core.rd8(iw(inf, w::STOCK_N) + 1);
    let cap = iw(inf, w::LIST_CAP);
    let mut n = 0;
    while n < n_stock && n < cap {
        core.wr(list + 4 * n, core.rd(stock + 4 * n));
        n += 1;
    }
    // the send effects first, then the master effects, each in row order (FX routing: the send effects are the ones the track FX key has a page for)
    let mut pass = 0;
    while pass < 2 {
        let mut k = 0;
        while k < iw(inf, w::PAGES) as usize && k < MAX_FX {
            if s.owner[k] != 0 && n < cap && fxsend::is_send(k) == (pass == 0) {
                core.wr(list + 4 * n, iw(inf, w::FIRST) + k as u32);
                n += 1;
            }
            k += 1;
        }
        pass += 1;
    }
    // no page of ours: the count 0 keeps the stock list (the hook in the builder)
    core.wr(iw(inf, w::COUNT), if n == n_stock { 0 } else { n });
}

/// Build the page list again (a route changed the order).
pub unsafe fn relist<C: CoreApi>(core: &C) {
    let inf = info(core);
    if !inf.is_null() {
        rebuild_list(core, inf);
    }
}

/// The module record + 1 that owns the page of effect row `k` (0: no page).
#[inline]
pub fn page_owner(k: usize) -> u8 {
    if k < MAX_FX {
        fs().owner[k]
    } else {
        0
    }
}

/// The tab label of the page of effect row `k` (up to 3 characters) into `out`.
#[inline]
pub fn tab_of(k: usize, out: &mut [u8; 8]) {
    if k < MAX_FX {
        let t = &fs().tab[k];
        let mut i = 0;
        while i < 8 && t[i] != 0 {
            out[i] = t[i];
            i += 1;
        }
    }
}
