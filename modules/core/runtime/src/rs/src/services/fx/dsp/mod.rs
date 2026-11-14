//! Effect-row part of the DSP loader: the chain rows, their owners, one-word writes and the clearing of a module's effect rows.
use crate::dsp::*;

/// Effect rows the image reserves for run-time effects (0 without `core/dynfx`).
pub unsafe fn fx_free_rows() -> u32 {
    if finfo().is_null() {
        0
    } else {
        fiw(F_FREE).min(MAX_FX as u32)
    }
}

/// Effect rows in use (claimed by a module that is loaded or loading), for CAPACITY.
pub unsafe fn fx_used() -> u32 {
    let d = ds();
    let mut used = 0;
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] != 0 {
            used += 1;
        }
        k += 1;
    }
    used
}

/// The module record + 1 that owns effect row `k` (0: free) and the row's index in the chain's table (the registry `fx`), for the effect pages.
pub unsafe fn fx_owner_of(k: usize) -> u8 {
    if k < MAX_FX {
        ds().fx_owner[k]
    } else {
        0
    }
}

/// The effect row (0 ..) that module record `owner - 1` claimed for its `ordinal`-th effect record (rows are claimed in record order, lowest first).
pub unsafe fn fx_row_of(owner: u8, ordinal: u32) -> Option<usize> {
    let d = ds();
    let mut seen = 0;
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] == owner {
            if seen == ordinal {
                return Some(k);
            }
            seen += 1;
        }
        k += 1;
    }
    None
}

/// What `init` declared for effect record `ordinal` of module `rec`: its route flags (`Some`) and the defaults of the words p0 to p7 (`Some`).
pub unsafe fn fx_set_declared(rec: usize, ordinal: u32, flags: Option<u32>, par: Option<[u32; 8]>) -> bool {
    match fx_row_of(rec as u8 + 1, ordinal) {
        Some(k) => {
            let d = ds();
            if let Some(f) = flags {
                d.fx_flags[k] = f;
            }
            if let Some(p) = par {
                d.fx_par[k] = p;
            }
            true
        }
        None => false,
    }
}

/// The flags word of the record that effect row `k` was claimed for (`FXF_SEND`, `FXF_NOMASTER`, `FXF_DEFAULT_SEND`; 0 for a free row).
pub unsafe fn fx_flags_of(k: usize) -> u32 {
    if k < MAX_FX {
        ds().fx_flags[k]
    } else {
        0
    }
}

/// Set the MIX word an effect row publishes (before its rows go out) and the defaults of its parameter words are the record's.
pub unsafe fn fx_set_mix(k: usize, bits: u32) {
    if k < MAX_FX {
        ds().fx_mix[k] = bits;
    }
}

/// Set the route and the four amount words an effect row publishes (before its rows go out; `fxsend::apply` does it from the settings).
pub unsafe fn fx_set_route(k: usize, route: u32, amt: [u32; 4]) {
    if k < MAX_FX {
        let d = ds();
        d.fx_route[k] = route;
        d.fx_amt[k] = amt;
    }
}

/// An effect row whose words are in the DSP: claimed, published and no load in progress, so a one-word write can go out.
pub unsafe fn fx_live(k: usize) -> bool {
    let d = ds();
    k < MAX_FX && d.fx_owner[k] != 0 && d.fx_pub[k] != 0 && d.phase == PH_IDLE && d.nwait == 0 && !crate::tracks::busy()
}

/// The effect row's words in the chain: word `w` of row `k` (a one-word write through window 2).
pub unsafe fn fx_word_start(k: usize, w: u32, value: u32) -> bool {
    if finfo().is_null() {
        return false;
    }
    start_word(WIN_FX, FX_ROW_WORDS * (fiw(F_FIRST) + k as u32) + w, value)
}

/// `Some(true)` done, `Some(false)` failed, `None` still running (a write started by `fx_word_start`).
pub unsafe fn fx_word_poll() -> Option<bool> {
    poll_done()
}

/// The module record (index) that owns row `r` of the effect table (the registry `fx`), for an effect fault.
pub unsafe fn module_of_fx_table_row(r: u32) -> Option<usize> {
    if finfo().is_null() {
        return None;
    }
    let first = fiw(F_FIRST);
    if r < first || ((r - first) as usize) >= MAX_FX {
        return None;
    }
    let owner = ds().fx_owner[(r - first) as usize];
    if owner == 0 {
        None
    } else {
        Some(owner as usize - 1)
    }
}

/// Free the effect rows module record `rec` published: the pc first, so the chain skips the row at once; then the adapter's words (`was`, `dis`, the fault
/// window and its count) back to 0, so the next effect on the row starts clean.
pub(crate) unsafe fn clear_fx_rows(rec: usize) -> bool {
    let d = ds();
    if finfo().is_null() {
        // Missing info is harmless only when no owned row was published.
        let mut k = 0;
        while k < MAX_FX {
            if d.fx_owner[k] == rec as u8 + 1 && d.fx_pub[k] != 0 { return false; }
            k += 1;
        }
        return true;
    }
    let first = fiw(F_FIRST);
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] == rec as u8 + 1 && d.fx_pub[k] != 0 {
            let at = FX_ROW_WORDS * (first + k as u32);
            if !write_word_blocking(WIN_FX, at + FXW_PC, FX_FREE_PC) { return false; }
            // the next effect on the row starts on the master route with no amounts (finding 62)
            if !write_word_blocking(WIN_FX, at + FXW_ROUTE, 0) { return false; }
            let mut w = 0;
            while w < 4 {
                if !write_word_blocking(WIN_FX, at + FXW_AMT0 + w, 0) { return false; }
                w += 1;
            }
            if !write_word_blocking(WIN_FX, at + FXW_WAS, 0) { return false; }
            if !write_word_blocking(WIN_FX, at + FXW_MIX, MIX_ONE) { return false; }
            if !write_word_blocking(WIN_FX, at + FXW_DIS, 0) { return false; }
            if !write_word_blocking(WIN_FX, at + FXW_WSTART, 0) { return false; }
            if !write_word_blocking(WIN_FX, at + FXW_WCOUNT, 0) { return false; }
            d.fx_pub[k] = 0;
        }
        k += 1;
    }
    true
}
