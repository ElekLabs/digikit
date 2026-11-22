//! Boot-time loading (docs/plans/digikit-runtime.md decision 9, Phase 6; finding 37).
//!
//! `dkrt_boot` runs once per boot from the main-OS task, just before the MainScreenView constructor (`src/cf/tramp.s`
//! `dkrt_boot_hook`): the block read, the DSP and the block-frame interrupt work there and the machine view is not built yet.
//! It reads the selection record, decides about safe mode, raises the boot counter, and loads the selected modules in order
//! through the same loader as ever (a module that fails is skipped and reported; the others load). DSP code is sent with
//! `dl_send` and awaited by polling the state machine here (bounded); what has not finished when the bound is reached continues
//! in the UI-loop hook as before. The boot counter is cleared by `ui_pass` once the session has been stable: `STABLE_BLOCKS` DSP blocks (30 s)
//! after the first UI-loop pass (at the first pass itself in a safe boot, where no module runs). A boot that dies before then counts as bad.
//!
//! The record is the only thing the runtime writes to the card here, and only to its two sectors (`store::Layout::sel_a`/`sel_b`), through
//! `block_write`, which refuses a write when the card's area is not ours (`card.rs`): then the boot counter is not raised either.

use crate::platform::st;
use crate::safe_mode::{key_held, DKRT_STABLE_BLOCKS, LATE_FLOOR};
use crate::selection::{current, update, Choice, REC, REC_COPY, REC_VALID};
use crate::{card, fault, dlog, loader, store};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_sdk::sel::{self, Record};
use dkrt_sdk::*;

pub use crate::safe_mode::*;
pub use crate::selection::*;


pub(crate) const INFO_MAGIC: u32 = 0x444b_4254; // "DKBT"

#[no_mangle]
pub static mut DKRT_BOOT: DkrtBootInfo = DkrtBootInfo {
    magic: 0,
    version: 1,
    safe: 0,
    reason: 0,
    counter: 0,
    seq: 0,
    copy: 0,
    count: 0,
    loaded: 0,
    ui_reached: 0,
    results: [0; SEL_MAX],
    dsp_wait: 0,
    stable: 0,
    needs: [[0; NEEDS_LEN]; SEL_MAX],
    states: [STATE_SELECTED; SEL_MAX],
    detail: [[0; 2]; SEL_MAX],
    unloads: 0,
    reloads: 0,
};

/// The file name of each module of this boot's selection (the part of its path after the last `/`, up to `NAME_LEN - 1` bytes, NUL padded): the LOADS
/// command reports it with the result, and the record may change after the boot (a SELECT).
pub use dkrt_abi::state::NAME_LEN;
static mut BOOT_NAME: [[u8; NAME_LEN]; SEL_MAX] = [[0; NAME_LEN]; SEL_MAX];

/// The file name of module `k` of this boot's selection (NUL padded, `NAME_LEN` bytes).
pub unsafe fn boot_name(k: usize) -> *const u8 {
    addr_of!(BOOT_NAME[k]) as *const u8
}

/// `dkrt_boot_name`: copy the name of index `k` to `out` (`NAME_LEN` bytes); `E_NOT_FOUND` past `SEL_MAX`.
pub unsafe fn name_copy(k: usize, out: *mut u8) -> i32 {
    if k >= SEL_MAX {
        return dkrt_sdk::E_NOT_FOUND;
    }
    let src = boot_name(k);
    let mut i = 0;
    while i < NAME_LEN {
        *out.add(i) = *src.add(i);
        i += 1;
    }
    dkrt_sdk::OK
}

/// The states of this boot's modules (`dkrt_abi::state`): a view of `DKRT_BOOT.states` and `.detail`.
pub fn states() -> dkrt_abi::state::BootStates<'static> {
    let b = info();
    dkrt_abi::state::BootStates { states: &mut b.states, detail: &mut b.detail }
}

/// The module records `LOADS` reports, as `dkrt_abi::state::LoadsView`.
pub fn loads_view() -> dkrt_abi::state::LoadsView<'static> {
    let b = info();
    dkrt_abi::state::LoadsView {
        count: b.count as usize,
        results: &b.results,
        states: &b.states,
        detail: &b.detail,
        needs: &b.needs,
        names: unsafe { &*addr_of!(BOOT_NAME) },
    }
}

/// Paths of the selection this boot loaded (for CAPACITY).
pub fn selected() -> u32 {
    info().count
}

pub fn info() -> &'static mut DkrtBootInfo {
    unsafe { &mut *addr_of_mut!(DKRT_BOOT) }
}

/// The boot hook (once per boot, main-OS task).
#[no_mangle]
pub unsafe extern "C" fn dkrt_boot() {
    crate::init_once();
    if !crate::dev::valid() {
        return; // a device object this runtime cannot read (a patch bug): load nothing, leave the firmware alone
    }
    let b = info();
    if b.magic == INFO_MAGIC {
        return; // the constructor ran once already: a second call is a bug, not a second boot
    }
    b.magic = INFO_MAGIC;
    b.version = 1;
    let (rec, copy, unreadable) = match current() {
        Choice::Use { record, copy, .. } => (Some(record), copy, false),
        Choice::Empty => (None, 0, false),
        Choice::Unreadable => (None, 0, true),
    };
    let valid = rec.is_some();
    let mut r = rec.unwrap_or(Record::empty());
    dlog::ev(logk::BOOT_START, dlog::INFO, [RUNTIME_VERSION, crate::device(), r.stable_blocks, r.seq, r.boot_counter]);
    // the card's area: ours, blank or neither (read-only), before anything is written
    card::check();
    // the settings record (the fx routes and send amounts): into RAM before the UI starts
    crate::settings::load();
    // the crash record and the RAM probe of the boot before
    fault::keep_probe();
    if fault::at_boot() {
        let seen = fault::seen();
        if seen.found != 0 && seen.stable != 0 {
            // the crashed boot had already cleared its counter: count it as bad now, and keep that one count until a boot with no crash
            // behind it has run 30 s (`ui_pass`)
            LATE_FLOOR = crate::safe_mode::count_late_crash(&mut r);
        }
    }
    b.seq = if valid { r.seq } else { 0 };
    b.copy = copy;
    b.counter = r.boot_counter;
    b.count = r.count;
    states().begin();
    REC = r;
    REC_VALID = valid;
    REC_COPY = copy;
    if r.stable_blocks != 0 {
        DKRT_STABLE_BLOCKS.0 = r.stable_blocks;
    }
    // safe mode: the record cannot be read, or it asked for it, or the key, or too many boots that did not reach the UI loop
    let reason = if unreadable {
        SAFE_READ
    } else if r.flags & sel::FLAG_SAFE_ONCE != 0 {
        SAFE_FLAG
    } else if key_held() {
        SAFE_KEY
    } else if fault::seen().hang != 0 {
        // the boot before stopped because the DSP stopped (a hang record): one stop is enough, even after a stable boot; the selection stays as it is
        SAFE_DSP
    } else if fault::seen().panic != 0 {
        // the boot before panicked (a panic record): one panic is enough, like a stop
        SAFE_PANIC
    } else if crate::safe_mode::counter_is_safe(&r) {
        SAFE_COUNTER
    } else {
        SAFE_NONE
    };
    dlog::ev(
        logk::BOOT_RECORD,
        dlog::INFO,
        [r.seq, copy, r.boot_counter, r.count, r.flags],
    );
    if reason != SAFE_NONE {
        b.safe = 1;
        b.reason = reason;
        LATE_FLOOR = 0;
        dlog::ev(logk::SAFE, logk::F_ERROR, [reason, r.boot_counter, r.seq, 0, 0]);
        if reason == SAFE_FLAG {
            // honoured once: clear the flag (the counter is left as it is; the UI loop clears it)
            r.flags &= !sel::FLAG_SAFE_ONCE;
            update(r, copy, valid);
        }
        // a safe boot loads no module: show the safe-mode screen in place of the stock intro (no storage access, built here on the main-OS task)
        crate::intro::show_safe(reason);
        // no provider loads and the installer writes nothing: every hook site stays stock
        loader::set_safe();
        let mut k = 0;
        while k < SEL_MAX {
            b.results[k] = E_SAFE_MODE;
            k += 1;
        }
        states().skip_all(r.count as usize);
        return;
    }
    // raise the counter before anything of a module runs; the stable period clears it. This counts from the first boot: with no record
    // yet (a fresh flash) the write creates one. On a card the runtime does not write there is no counter, so no safe mode by counter
    // (ABOUT says so).
    r = crate::safe_mode::count_boot_with(&crate::PLATFORM, r, copy, valid, card::writable());
    // The frames that carry DSP code do not flow yet (they start with the sound engine, later in the task), so a module's CF part
    // goes live now and its DSP part follows over the UI-loop passes (`LOADING` in the report until then).
    // the header table of every file of the selection, before the first load (reads the files; the staging buffer is free): the plan reads them
    read_tables(&r);
    // the boot plan over the header tables that read gave, with the providers the search found for the hard needs the runtime does not meet: a module the plan refuses is
    // not loaded
    let planned = loader::plan_selection(r.count as usize);
    // the package manager loads the accepted modules and the services they need (services first), reports each module in plan order and the providers in the log
    crate::lifecycle::boot(&r, planned);
    // core/introblank: no module took the intro, so the black intro has nothing to wait for
    crate::intro::end_blank();
}

/// Read the file of each selected module into the staging buffer and keep its header table for the boot plan (`loader::capture`). A file that is missing, torn or
/// not a `.dkm` has none (its load reports the error).
unsafe fn read_tables(rec: &Record) {
    loader::reset_plan();
    let mut k = 0;
    while k < rec.count as usize {
        if let Ok(found) = store::find(rec.paths[k].as_ptr()) {
            if store::load_to_stage(&found) == OK {
                let file = core::slice::from_raw_parts(store::stage() as *const u8, found.size as usize);
                loader::capture(k, file);
            }
        }
        k += 1;
    }
}

/// A 32-bit hash of each index's full selection path at this boot (0: no path; an index that a live load added past the boot's count has none). Written once, by the boot's
/// load loop (`note_boot_path`); a live load never writes it, so an index stays the index its path had at the boot, whichever module used it in between.
static mut BOOT_PATH: [u32; SEL_MAX] = [0; SEL_MAX];

/// The hash of a store path (the bytes up to the NUL): FNV-1a, never 0.
pub unsafe fn path_hash(path: *const u8) -> u32 {
    let n = crate::cstr_len(path, 64);
    let h = dkneeds::keys::name_hash_bytes(core::slice::from_raw_parts(path, n));
    if h == 0 {
        1
    } else {
        h
    }
}

/// The boot's load loop reads selection entry `k` with path `path`: its index belongs to that path from now on.
pub unsafe fn note_boot_path(k: usize, path: *const u8) {
    if k < SEL_MAX {
        BOOT_PATH[k] = path_hash(path);
    }
}

/// The selection index a run-time load of the file with path hash `hash` takes (docs/plans/extension-model.md E5c): the index that path had at this boot, when no module lives
/// there now (`live`); else the first index below `count` that no module lives at and no boot path owns (an index an earlier live load added); else a new index (`count`).
/// `E_SEL_COUNT` when all `SEL_MAX` are taken. An unloaded boot module keeps its index for itself.
pub fn pick_index(hash: u32, boot: &[u32; SEL_MAX], count: usize, live: &[bool; SEL_MAX]) -> Result<usize, i32> {
    let count = count.min(SEL_MAX);
    let mut k = 0;
    while k < count {
        if !live[k] && hash != 0 && boot[k] == hash {
            return Ok(k);
        }
        k += 1;
    }
    k = 0;
    while k < count {
        if !live[k] && boot[k] == 0 {
            return Ok(k);
        }
        k += 1;
    }
    if count < SEL_MAX {
        Ok(count)
    } else {
        Err(E_SEL_COUNT)
    }
}

/// `pick_index` over this boot's record for the file at `path`.
pub unsafe fn live_index(path: *const u8, live: &[bool; SEL_MAX]) -> Result<usize, i32> {
    pick_index(path_hash(path), &*addr_of!(BOOT_PATH), info().count as usize, live)
}

/// The boot record's row of the index a live job started to load into, as it was before (`begin_live`): a load that fails puts it back.
struct Saved {
    k: usize,
    count: u32,
    state: u8,
    result: i32,
    needs: [u8; NEEDS_LEN],
    detail: [u32; 2],
    name: [u8; NAME_LEN],
}
static mut SAVED: Saved = Saved { k: SEL_MAX, count: 0, state: 0, result: 0, needs: [0; NEEDS_LEN], detail: [0; 2], name: [0; NAME_LEN] };

/// A live load of index `k` failed: the boot record is as before the job (an UNLOADED module stays UNLOADED, an index that was never used is SELECTED again and the count
/// is the old one), so a failed load leaves no FAILED row and no name behind. Only a load calls it: a reload that fails leaves the module FAILED.
pub unsafe fn restore_live(k: usize) {
    if k >= SEL_MAX || SAVED.k != k {
        return;
    }
    let b = info();
    b.count = SAVED.count;
    b.states[k] = SAVED.state;
    b.results[k] = SAVED.result;
    b.needs[k] = SAVED.needs;
    b.detail[k] = SAVED.detail;
    BOOT_NAME[k] = SAVED.name;
    SAVED.k = SEL_MAX;
}

/// A live job starts the load of selection index `k` (a load into a free index, or the load half of a reload): the module is INIT, the loaded count drops if it was ACTIVE,
/// an index past the boot's count joins it, and the result words of the last attempt are cleared.
pub unsafe fn begin_live(k: usize) {
    let b = info();
    SAVED = Saved { k, count: b.count, state: b.states[k], result: b.results[k], needs: b.needs[k], detail: b.detail[k], name: BOOT_NAME[k] };
    if b.states[k] == STATE_ACTIVE && b.loaded > 0 {
        b.loaded -= 1;
    }
    if k >= b.count as usize {
        b.count = k as u32 + 1;
    }
    b.states[k] = STATE_INIT;
    b.results[k] = OK;
    b.needs[k] = [0; NEEDS_LEN];
    b.detail[k] = [0; 2];
}

/// Module `k` was taken down by an unload: UNLOADED, and the loaded count drops if it was ACTIVE.
pub unsafe fn unloaded(k: usize) {
    if k >= SEL_MAX {
        return;
    }
    let b = info();
    if b.states[k] == STATE_ACTIVE && b.loaded > 0 {
        b.loaded -= 1;
    }
    b.states[k] = STATE_UNLOADED;
    b.results[k] = OK;
}

/// The name a module event shows (`LOG_NAME` bytes): the name part of the module id when the loader read the header (`name`, not empty), else the file leaf without its `.dkm`.
pub fn shown_name(leaf: &[u8], name: &[u8; crate::loader::LOG_NAME]) -> [u8; crate::loader::LOG_NAME] {
    if name[0] != 0 {
        return *name;
    }
    let leaf = leaf.strip_suffix(b".dkm").unwrap_or(leaf);
    let mut out = [0u8; crate::loader::LOG_NAME];
    let n = leaf.len().min(crate::loader::LOG_NAME);
    out[..n].copy_from_slice(&leaf[..n]);
    out
}

/// The result `res` of the load of selection entry `k` (the file at `path`): the boot record, the log events and the name the LOADS report shows.
pub unsafe fn report_module(k: usize, path: *const u8, res: i32, missing: &[u8; NEEDS_LEN], name: &[u8; crate::loader::LOG_NAME]) {
    let b = info();
    st().last_result = res;
    b.results[k] = res;
    if res == OK {
        b.loaded += 1;
    }
    let p = path;
    let mut at = 0;
    let mut i = 0;
    while i < SEL_PATH && *p.add(i) != 0 {
        if *p.add(i) == b'/' {
            at = i + 1;
        }
        i += 1;
    }
    let bad = if res == OK || res == LOADING { dlog::INFO } else { logk::F_ERROR };
    let mut leaf_len = 0;
    while at + leaf_len < SEL_PATH && *p.add(at + leaf_len) != 0 {
        leaf_len += 1;
    }
    let shown = shown_name(core::slice::from_raw_parts(p.add(at), leaf_len), name);
    let w = crate::loader::LOG_NAME;
    dlog::ev(
        logk::MOD_LOAD,
        bad,
        [k as u32, res as u32, dlog::word_of(shown.as_ptr(), 0, w), dlog::word_of(shown.as_ptr(), 4, w), dlog::word_of(shown.as_ptr(), 8, w)],
    );
    let mut j = 0;
    while j < NAME_LEN - 1 && at + j < SEL_PATH && *p.add(at + j) != 0 {
        BOOT_NAME[k][j] = *p.add(at + j);
        j += 1;
    }
    if res == E_IMPORT && missing[0] != 0 {
        let m = *missing;
        b.needs[k] = m;
        dlog::ev(
            logk::MOD_NEEDS,
            logk::F_ERROR,
            [k as u32, dlog::word_of(m.as_ptr(), 0, NEEDS_LEN), dlog::word_of(m.as_ptr(), 4, NEEDS_LEN), dlog::word_of(m.as_ptr(), 8, NEEDS_LEN), 0],
        );
    }
}

/// The result `res` of the load of the provider file at the store path `path` (`services/<name>.dks`) that provides the service `hash`, loaded at `base` (0 when
/// it did not load): log only. The event is `MOD_LOAD` with the provider index (`loader::PROVIDER_IDX`, not a selection index), the result, the first four
/// characters of the file name, the service hash and the base; the boot record and `LOADS` do not change.
pub unsafe fn report_provider(path: &[u8], res: i32, hash: u32, base: u32) {
    let mut at = 0;
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'/' {
            at = i + 1;
        }
        i += 1;
    }
    let mut name = [0u8; 4];
    let mut j = 0;
    while j < 4 && at + j < path.len() {
        name[j] = path[at + j];
        j += 1;
    }
    let bad = if res == OK { dlog::INFO } else { logk::F_ERROR };
    dlog::ev(
        logk::MOD_LOAD,
        bad,
        [loader::PROVIDER_IDX, res as u32, dlog::word_of(name.as_ptr(), 0, 4), hash, base],
    );
}

/// The DSP part of the module loaded for selection index `idx` is over with result `r` (called by the DSP loader).
pub unsafe fn report_dsp(idx: u32, r: i32) {
    dlog::ev(logk::MOD_DSP, if r == OK { dlog::INFO } else { logk::F_ERROR }, [idx, r as u32, 0, 0, 0]);
    if (idx as usize) < SEL_MAX {
        // only a module that waits for its DSP part takes the result; an active or failed module keeps its record
        if states().dsp_done(idx as usize, r) {
            let b = info();
            b.results[idx as usize] = r;
            if r == OK {
                b.loaded += 1;
            }
        }
    }
}

/// Module `idx` of the selection was taken down because a service it needs failed (late teardown of a service whose DSP part failed): FAILED with `code`
/// (`E_MISSING`), the count of loaded modules corrected, one `MOD_DSP` event like the end of a DSP part.
pub unsafe fn report_taken_down(idx: u32, code: i32) {
    if (idx as usize) < SEL_MAX {
        let b = info();
        let was_active = states().states[idx as usize] == dkrt_sdk::STATE_ACTIVE;
        if states().taken_down(idx as usize) {
            if was_active && b.loaded > 0 {
                b.loaded -= 1;
            }
            b.results[idx as usize] = code;
        }
    }
    dlog::ev(logk::MOD_DSP, logk::F_ERROR, [idx, code as u32, 0, 0, 0]);
}

pub unsafe fn boot_info(out: *mut DkrtBootInfo) {
    let b = info();
    let o = &mut *out;
    *o = DkrtBootInfo {
        magic: b.magic,
        version: b.version,
        safe: b.safe,
        reason: b.reason,
        counter: b.counter,
        seq: b.seq,
        copy: b.copy,
        count: b.count,
        loaded: b.loaded,
        ui_reached: b.ui_reached,
        results: b.results,
        dsp_wait: b.dsp_wait,
        stable: b.stable,
        needs: b.needs,
        states: b.states,
        detail: b.detail,
        unloads: b.unloads,
        reloads: b.reloads,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::LOCK;
    use dkrt_abi::plan::{Reason, Verdict};

    #[test]
    fn the_boot_record_feeds_the_loads_reply_and_the_boot_info_copy() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            *info() = core::mem::zeroed();
            info().count = 2;
            states().begin();
            states().planned(0, Verdict::Accepted);
            states().init(0);
            states().finished(0, OK);
            states().planned(1, Verdict::Refused(Reason::DuplicateKey(7, 99)));
            info().results[1] = E_PLAN_KEY;
            BOOT_NAME[0][..5].copy_from_slice(b"a.dkm");
            let mut out = [0u8; dkrt_abi::state::loads_len(SEL_MAX)];
            let n = dkrt_abi::state::loads_v2(&loads_view(), &mut out);
            assert_eq!(n, dkrt_abi::state::loads_len(2));
            assert_eq!(out[..2], [2, 2]);
            assert_eq!(out[2 + 4], STATE_ACTIVE);
            let e1 = 2 + dkrt_abi::state::LOADS_ENTRY;
            assert_eq!((out[e1 + 4], out[e1 + 11], out[e1 + 15]), (STATE_REFUSED, 7, 99));
            let mut copy: DkrtBootInfo = core::mem::zeroed();
            boot_info(&mut copy);
            assert_eq!((copy.states[..2].to_vec(), copy.detail[1]), (vec![STATE_ACTIVE, STATE_REFUSED], [7, 99]));
        }
    }
    #[test]
    fn boot_name_copies_the_name_of_an_index_and_refuses_one_past_the_selection() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            fresh_boot_record();
            BOOT_NAME[1][..5].copy_from_slice(b"b.dkm");
            let mut out = [0xffu8; NAME_LEN];
            assert_eq!(name_copy(1, out.as_mut_ptr()), OK);
            assert_eq!(&out[..6], b"b.dkm\0");
            assert_eq!(out[NAME_LEN - 1], 0);
            assert_eq!(name_copy(SEL_MAX, out.as_mut_ptr()), E_NOT_FOUND);
        }
    }

    /// `path` as the NUL-terminated selection path the loader hands to `report_module`.
    fn sel_path(p: &[u8]) -> [u8; SEL_PATH] {
        let mut out = [0u8; SEL_PATH];
        out[..p.len()].copy_from_slice(p);
        out
    }

    unsafe fn name_of(k: usize) -> [u8; NAME_LEN] {
        *addr_of!(BOOT_NAME[k])
    }

    unsafe fn fresh_boot_record() {
        *info() = core::mem::zeroed();
        BOOT_NAME = [[0; NAME_LEN]; SEL_MAX];
        states().begin();
    }

    #[test]
    fn a_module_event_shows_the_module_name_when_the_header_was_read_else_the_leaf_without_dkm() {
        let id = |s: &[u8]| {
            let mut n = [0u8; crate::loader::LOG_NAME];
            n[..s.len()].copy_from_slice(s);
            n
        };
        // the header was read: the name part of the id, whatever the file is called
        assert_eq!(shown_name(b"digikit-usb-fast.dkm", &id(b"usb-fast")), id(b"usb-fast"));
        // no header (file missing, unreadable): the leaf without `.dkm`, cut to the log's width
        assert_eq!(shown_name(b"digikit-usb-fast.dkm", &[0; crate::loader::LOG_NAME]), id(b"digikit-usb-"));
        assert_eq!(shown_name(b"gone.dkm", &[0; crate::loader::LOG_NAME]), id(b"gone"));
        assert_eq!(shown_name(b"noext", &[0; crate::loader::LOG_NAME]), id(b"noext"));
    }

    #[test]
    fn a_module_report_keeps_the_file_leaf_and_names_the_missing_service_on_an_import_error() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            fresh_boot_record();
            let mut missing = [0u8; NEEDS_LEN];
            missing[..8].copy_from_slice(b"usb-bulk");
            report_module(1, sel_path(b"mods/deep/a.dkm").as_ptr(), E_IMPORT, &missing, &[0; crate::loader::LOG_NAME]);
            assert_eq!(info().results[1], E_IMPORT);
            assert_eq!(st().last_result, E_IMPORT);
            assert_eq!(info().loaded, 0, "only OK counts as loaded");
            assert_eq!(info().needs[1], missing);
            assert_eq!(name_of(1)[..6], *b"a.dkm\0");
            assert_eq!(info().needs[0], [0; NEEDS_LEN]);

            // another error with a name set keeps no needs; an import error with no name keeps none either
            fresh_boot_record();
            report_module(0, sel_path(b"a.dkm").as_ptr(), E_INIT, &missing, &[0; crate::loader::LOG_NAME]);
            report_module(1, sel_path(b"b.dkm").as_ptr(), E_IMPORT, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            assert_eq!(info().needs, [[0; NEEDS_LEN]; SEL_MAX]);
            assert_eq!(info().results[..2], [E_INIT, E_IMPORT]);

            // OK counts, LOADING does not (the DSP part reports later), and a leaf over the name field is cut with its NUL kept
            fresh_boot_record();
            report_module(0, sel_path(b"x/ok.dkm").as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            report_module(1, sel_path(b"x/dsp.dkm").as_ptr(), LOADING, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            assert_eq!(info().loaded, 1);
            let mut long = b"mods/".to_vec();
            long.extend_from_slice(&[b'n'; 40]);
            long.extend_from_slice(b".dkm");
            report_module(2, sel_path(&long).as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            assert_eq!(name_of(2)[..NAME_LEN - 1], [b'n'; NAME_LEN - 1]);
            assert_eq!(name_of(2)[NAME_LEN - 1], 0);
        }
    }

    #[test]
    fn a_provider_report_is_a_log_event_only_and_a_dsp_report_updates_one_row() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            fresh_boot_record();
            info().count = 2;
            for k in 0..2 {
                states().planned(k, Verdict::Accepted);
                states().init(k);
                states().finished(k, LOADING);
            }
            let (results, states_before, loaded) = (info().results, info().states, info().loaded);
            report_provider(b"services/lfo.dks", OK, 0x1234, 0x0010_0000);
            report_provider(b"services/x", E_FORMAT, 0, 0);
            assert_eq!((info().results, info().states, info().loaded), (results, states_before, loaded));
            assert_eq!(*addr_of!(BOOT_NAME), [[0; NAME_LEN]; SEL_MAX]);

            report_dsp(0, OK);
            assert_eq!((info().results[0], info().states[0], info().loaded), (OK, STATE_ACTIVE, 1));
            assert_eq!((info().results[1], info().states[1]), (0, STATE_DSP_LOADING), "the other row is untouched");
            report_dsp(1, E_INIT);
            assert_eq!((info().results[1], info().states[1], info().loaded), (E_INIT, STATE_FAILED, 1));
            // an index past the selection changes nothing
            let before = (info().results, info().states, info().loaded);
            report_dsp(SEL_MAX as u32, E_INIT);
            assert_eq!((info().results, info().states, info().loaded), before);
        }
    }

    #[test]
    fn a_dsp_report_for_a_module_that_is_not_waiting_for_its_dsp_changes_nothing() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            fresh_boot_record();
            info().count = 2;
            for k in 0..2 {
                states().planned(k, Verdict::Accepted);
                states().init(k);
            }
            // module 0 is active (no DSP part); module 1 failed in its init
            states().finished(0, OK);
            report_module(0, sel_path(b"a.dkm").as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            states().finished(1, E_INIT);
            report_module(1, sel_path(b"b.dkm").as_ptr(), E_INIT, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            let before = (info().results, info().states, info().loaded);
            report_dsp(0, E_INIT);
            report_dsp(1, OK);
            assert_eq!((info().results, info().states, info().loaded), before);
        }
    }

    fn h(p: &str) -> u32 {
        let mut v = p.as_bytes().to_vec();
        v.push(0);
        unsafe { path_hash(v.as_ptr()) }
    }

    fn boot_of(list: &[&str]) -> [u32; SEL_MAX] {
        let mut out = [0u32; SEL_MAX];
        for (k, p) in list.iter().enumerate() {
            out[k] = h(p);
        }
        out
    }

    #[test]
    fn a_live_load_takes_the_index_its_path_had_at_the_boot_else_a_free_one_no_boot_path_owns_else_a_new_one() {
        let boot = boot_of(&["modules/a.dkm", "modules/b.dkm", "modules/c.dkm"]);
        let mut live = [false; SEL_MAX];
        // the path was in the boot's selection and no module lives there
        assert_eq!(pick_index(h("modules/b.dkm"), &boot, 3, &live), Ok(1));
        // a module lives at that index (it cannot be another file, a boot index belongs to its path): the file takes a new index, not another boot path's
        live[1] = true;
        assert_eq!(pick_index(h("modules/b.dkm"), &boot, 3, &live), Ok(3), "the other boot indices belong to their paths");
        assert_eq!(pick_index(h("modules/new.dkm"), &boot, 3, &[false; SEL_MAX]), Ok(3), "an unknown file does not take the index of an unloaded boot module");
        // an index an earlier live load added is free for any file once its module is gone
        let mut count4 = boot;
        count4[3] = 0;
        assert_eq!(pick_index(h("modules/new.dkm"), &count4, 4, &[false; SEL_MAX]), Ok(3));
        assert_eq!(pick_index(h("modules/new.dkm"), &count4, 4, &{
            let mut l = [false; SEL_MAX];
            l[3] = true;
            l
        }), Ok(4));
        // the table is full
        let full = [true; SEL_MAX];
        assert_eq!(pick_index(h("modules/new.dkm"), &boot_of(&[]), SEL_MAX, &full), Err(E_SEL_COUNT));
        assert_eq!(pick_index(h("modules/new.dkm"), &[1; SEL_MAX], SEL_MAX + 3, &[false; SEL_MAX]), Err(E_SEL_COUNT), "a count past the table is cut to it; every index belongs to a boot path");
        // an empty boot selection: the first index
        assert_eq!(pick_index(h("modules/x.dkm"), &boot_of(&[]), 0, &[false; SEL_MAX]), Ok(0));
    }

    #[test]
    fn a_module_keeps_its_boot_index_after_another_used_it_and_long_leaves_with_one_prefix_do_not_collide() {
        // A at 0 and B at 1 at the boot. A is unloaded, C loads (a new index, 2, since 0 belongs to A), C is unloaded: A loads again at 0
        let boot = boot_of(&["modules/a.dkm", "modules/b.dkm"]);
        let mut live = [false, true, false, false, false, false, false, false];
        let c = pick_index(h("modules/c.dkm"), &boot, 2, &live).unwrap();
        assert_eq!(c, 2);
        live[c] = true;
        live[c] = false;
        assert_eq!(pick_index(h("modules/a.dkm"), &boot, 3, &live), Ok(0), "A is back at the index it had");
        // taxonomy leaves of 35 characters that differ in the last characters only (the old name match cut them at 27)
        let (x, y) = ("modules/machines/digikit-long-machine-name-1.dkm", "modules/machines/digikit-long-machine-name-2.dkm");
        let boot = boot_of(&[x, y]);
        assert_eq!(pick_index(h(y), &boot, 2, &[false; SEL_MAX]), Ok(1));
        assert_eq!(pick_index(h(x), &boot, 2, &[false; SEL_MAX]), Ok(0));
        assert_ne!(h(x), h(y));
    }

    #[test]
    fn the_boot_paths_are_noted_once_by_the_boot_and_a_live_load_never_changes_them() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            BOOT_PATH = [0; SEL_MAX];
            fresh_boot_record();
            info().count = 2;
            note_boot_path(0, b"modules/a.dkm\0".as_ptr());
            note_boot_path(1, b"modules/b.dkm\0".as_ptr());
            note_boot_path(SEL_MAX, b"x\0".as_ptr());
            // a live load of another file into index 1 (it was unloaded) overwrites the display name and nothing else
            begin_live(1);
            report_module(1, sel_path(b"modules/c.dkm").as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            assert_eq!(&name_of(1)[..5], b"c.dkm");
            assert_eq!(BOOT_PATH[..2], [h("modules/a.dkm"), h("modules/b.dkm")]);
            let mut live = [false; SEL_MAX];
            live[1] = true;
            assert_eq!(live_index(b"modules/c.dkm\0".as_ptr(), &live), Ok(2));
            live[1] = false;
            assert_eq!(live_index(b"modules/b.dkm\0".as_ptr(), &live), Ok(1), "b comes back to its own index");
        }
    }

    #[test]
    fn a_failed_live_load_puts_the_boot_record_back() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            // an unloaded module stays UNLOADED, with its name and result
            fresh_boot_record();
            info().count = 1;
            states().planned(0, Verdict::Accepted);
            states().init(0);
            states().finished(0, OK);
            report_module(0, sel_path(b"modules/a.dkm").as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            unloaded(0);
            let before = (info().count, info().states[0], info().results[0], info().loaded, name_of(0));
            begin_live(0);
            states().finished(0, E_INIT);
            report_module(0, sel_path(b"modules/other.dkm").as_ptr(), E_INIT, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            restore_live(0);
            assert_eq!((info().count, info().states[0], info().results[0], info().loaded, name_of(0)), before);
            // an index that was never used leaves no FAILED row, no name and no count
            begin_live(1);
            assert_eq!(info().count, 2);
            states().finished(1, E_INIT);
            report_module(1, sel_path(b"modules/new.dkm").as_ptr(), E_INIT, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            restore_live(1);
            assert_eq!((info().count, info().states[1], info().results[1], name_of(1)), (1, STATE_SELECTED, OK, [0; NAME_LEN]));
        }
    }

    #[test]
    fn a_live_job_moves_the_boot_record_through_init_active_and_unloaded() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            fresh_boot_record();
            info().count = 2;
            for k in 0..2 {
                states().planned(k, Verdict::Accepted);
                states().init(k);
                states().finished(k, OK);
                report_module(k, sel_path(b"a.dkm").as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            }
            assert_eq!((info().loaded, info().states[..2].to_vec()), (2, vec![STATE_ACTIVE, STATE_ACTIVE]));
            // an unload: UNLOADED, the loaded count drops once, the other module is untouched; a second one does not drop it again
            unloaded(0);
            assert_eq!((info().loaded, info().states[..2].to_vec(), info().results[0]), (1, vec![STATE_UNLOADED, STATE_ACTIVE], OK));
            unloaded(0);
            assert_eq!(info().loaded, 1);
            unloaded(SEL_MAX);
            // a load into the free index, and one into a new index that joins the count
            info().needs[0] = [7; NEEDS_LEN];
            info().results[0] = E_IMPORT;
            begin_live(0);
            assert_eq!((info().states[0], info().results[0], info().needs[0], info().count), (STATE_INIT, OK, [0; NEEDS_LEN], 2));
            begin_live(2);
            assert_eq!((info().states[2], info().count), (STATE_INIT, 3));
            assert!(states().finished(2, OK) && states().finished(0, LOADING));
            report_module(2, sel_path(b"new.dkm").as_ptr(), OK, &[0; NEEDS_LEN], &[0; crate::loader::LOG_NAME]);
            assert_eq!((info().loaded, info().states[..3].to_vec()), (2, vec![STATE_DSP_LOADING, STATE_ACTIVE, STATE_ACTIVE]));
            assert_eq!(name_of(2)[..8], *b"new.dkm\0");
            // the load half of a reload takes an active module to INIT and drops the count
            begin_live(1);
            assert_eq!((info().loaded, info().states[1]), (1, STATE_INIT));
        }
    }
}
