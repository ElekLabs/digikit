//! Register and remove the send cell of an effect row.

use super::*;
use crate::dsp as fxdsp;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;
use dkrt_abi::mem::{cstr_len, iw};

/// Effect row `k` (module record `owner - 1`, tab string `tab`) has been registered: read its route and amounts from the settings and, for a send effect within the limit, give it a
/// cell of the send page. The route and the four amount words go to the DSP with the row (`dsp::fx_set_route`).
pub unsafe fn apply<C: CoreApi>(core: &C, k: usize, owner: u8, tab: *const u8) {
    apply_row(core, k, owner, tab);
    publish(core);
}

unsafe fn apply_row<C: CoreApi>(core: &C, k: usize, owner: u8, tab: *const u8) {
    if k >= dsp::MAX_FX || owner == 0 {
        return;
    }
    let s = ss();
    let inf = info(core);
    if !inf.is_null() {
        release(core, inf, k);
    }
    let (allow, dflt) = routes_of(fxdsp::fx_flags_of(core, k));
    s.allow[k] = allow;
    s.dflt[k] = dflt;
    let name = core.module_name(owner);
    let mut n = 0;
    while n < 31 && *name.add(n) != 0 {
        s.id[k][n] = *name.add(n);
        n += 1;
    }
    while n < 32 {
        s.id[k][n] = 0;
        n += 1;
    }
    s.idlen[k] = cstr_len(name, 32) as u8;
    // an image with no send page (no cells) has every effect on the master, whatever the settings say
    if inf.is_null() {
        return;
    }
    let want = saved_route(core, k) == ROUTE_SEND;
    let mut key = [0u8; KEY_MAX];
    let mut a = [0u8; TRACKS];
    let kn = key_of(&mut key, b"fx/send/", &s.id[k], s.idlen[k] as usize);
    if core.settings_get(&key[..kn], &mut a) != TRACKS as i32 {
        a = [0u8; TRACKS];
    }
    let mut t = 0;
    while t < TRACKS {
        if a[t] > 127 {
            a[t] = 127;
        }
        t += 1;
    }
    s.amt[k] = a;
    s.dsp[k] = words_of(&a);
    s.want[k] = s.dsp[k];
    let cells = iw(inf, w::CELLS).min(SEND_MAX as u32) as usize;
    let mut c = 0;
    while c < cells && s.row[c] != 0 {
        c += 1;
    }
    if want && c < cells {
        s.route[k] = 1;
        s.cell[k] = c as u8 + 1;
        s.row[c] = k as u8 + 1;
        s.used += 1;
        // the knob: label "<tab> Send", short name the tab; the value class of the Delay rows, the owner of the stock send row, the Sound index of the cell
        let mut i = 0;
        let mut name = [0u8; 16];
        while i < 3 && *tab.add(i) != 0 {
            name[i] = *tab.add(i);
            i += 1;
        }
        let tl = i;
        copy(&mut s.short[c], name.as_ptr(), tl);
        let suffix = b" Send";
        i = 0;
        while i < suffix.len() {
            name[tl + i] = suffix[i];
            i += 1;
        }
        copy(&mut s.name[c], name.as_ptr(), 16);
        let code = iw(inf, w::P_SEND) + c as u32;
        dkrt_svc_rows::write_send_param_row(
            core,
            dkrt_svc_rows::dynreg(core),
            code,
            iw(inf, w::TPL_PERCENT),
            iw(inf, w::TEMPLATE),
            iw(inf, w::IDX0 + c),
            s.name[c].as_ptr() as u32,
            s.short[c].as_ptr() as u32,
        );
        core.wr(iw(inf, w::ROW) + 8 + 4 * c as u32, code);
        // every Sound array gets the stored amounts before it is read
        s.restore = 0xffff;
        s.kit = 0;
        publish_count(core, inf);
    }
    fxdsp::fx_set_route(core, k, s.route[k] as u32, s.dsp[k]);
    // the FX view lists the send effects first: the list is built again now that the route is known
    fxpage::relist(core);
}

/// Give back the cell of effect row `k` (if it has one).
unsafe fn release<C: CoreApi>(core: &C, inf: *const u32, k: usize) {
    let s = ss();
    if s.route[k] == 0 {
        return;
    }
    let c = s.cell[k] as usize - 1;
    core.wr(iw(inf, w::ROW) + 8 + 4 * c as u32, 0);
    dkrt_svc_rows::reset_param_row(
        core,
        dkrt_svc_rows::dynreg(core),
        iw(inf, w::P_SEND) + c as u32,
    );
    s.row[c] = 0;
    s.cell[k] = 0;
    s.route[k] = 0;
    s.used -= 1;
    let mut t = 0;
    while t < TRACKS {
        s.shown[c][t] = -1;
        t += 1;
    }
    publish_count(core, inf);
}

/// The effect row `k` is gone (its module was removed): its cell and its state.
pub unsafe fn remove<C: CoreApi>(core: &C, k: usize) {
    remove_row(core, k);
    publish(core);
}

unsafe fn remove_row<C: CoreApi>(core: &C, k: usize) {
    if k >= dsp::MAX_FX {
        return;
    }
    let inf = info(core);
    if !inf.is_null() {
        release(core, inf, k);
    }
    fxdsp::fx_forget(core, k);
    let s = ss();
    s.allow[k] = 0;
    s.dflt[k] = 0;
    s.amt[k] = [0; TRACKS];
    s.dsp[k] = [0; 4];
    s.want[k] = [0; 4];
    s.idlen[k] = 0;
    s.id[k] = [0; 32];
}
