//! The `.dkm` loader (docs/design/digikit-dkm.md). Every check runs before anything is written to the arena.

use crate::dsp;
use crate::sha256::Sha;
use crate::entry::Ctx;
use crate::{
    arena, cstr_eq, cstr_len, flush, import_addr, rd32, st, store, wr32, Module,
    MAX_MODULES,
};
use core::ptr::addr_of_mut;
use dkrt_sdk::*;

const MAGIC: u32 = 0x444b_4d46; // "DKMF"
/// The header size. `init`, `fini`, `menu` and `tick` are image offsets in it (NONE for a missing one): the fields of the module's `dk_module` constant
/// (`dk_module_def`), called with a `dk_module_ctx *` (`entry.rs`).
const HEADER: u32 = 0xa0;
const NONE: u32 = 0xffff_ffff;

// header field offsets (big endian)
const H_SIZE: usize = 0x08;
const H_HDRSIZE: usize = 0x0c;
const H_SHA: usize = 0x10;
#[allow(dead_code)]
const H_RUNTIME: usize = 0x30; // informational since runtime 0.1.0
const H_VERSION: usize = 0x34;
const H_CONTRACT: usize = 0x38; // api, machine, fx, service: (major, minor) bytes
const H_NAME: usize = 0x40;
const H_IMAGE_OFF: usize = 0x60;
const H_IMAGE_SIZE: usize = 0x64;
const H_BSS: usize = 0x68;
const H_ALIGN: usize = 0x6c;
const H_RELOC_OFF: usize = 0x70;
const H_RELOC_N: usize = 0x74;
const H_IMPORT_OFF: usize = 0x78;
const H_IMPORT_N: usize = 0x7c;
const H_INIT: usize = 0x80;
const H_FINI: usize = 0x84;
const H_MENU: usize = 0x88;
const H_TICK: usize = 0x8c;
const H_REG_OFF: usize = 0x90;
const H_REG_SIZE: usize = 0x94;
const H_DSP_OFF: usize = 0x98;
const H_DSP_SIZE: usize = 0x9c;

unsafe fn in_file(off: u32, len: u32, size: u32) -> bool {
    off <= size && len <= size - off
}

/// The machine keys (first word of each MACHINE record of the `register` section) of the `.dkm` at `path`, for the boot's type assignment
/// (`assign::plan`): at most 8, the count returned. Reads the file into the staging buffer, so it runs before the first load. A file that is
/// missing, torn or not a `.dkm` has none (its load reports the error).
pub unsafe fn machine_keys(path: *const u8, out: &mut [u32; 8]) -> usize {
    let found = match store::find(path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    if store::load_to_stage(&found) != OK {
        return 0;
    }
    let f = store::stage() as *const u8;
    let size = found.size;
    if size < HEADER || rd32(f) != MAGIC || rd32(f.add(H_SIZE)) != size {
        return 0;
    }
    let (off, len) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
    if len < 16 || !in_file(off, len, size) {
        return 0;
    }
    let reg = f.add(off as usize);
    if rd32(reg) != 0x444b_5247 {
        return 0;
    }
    let (n, mut at) = (rd32(reg.add(12)), 16u32);
    let mut found_n = 0;
    let mut i = 0;
    while i < n {
        if len - at < 8 {
            break;
        }
        let (kind, rl) = (rd32(reg.add(at as usize)), rd32(reg.add(at as usize + 4)));
        if rl < 12 || rl > len - at {
            break;
        }
        if kind == 2 && found_n < 8 {
            out[found_n] = rd32(reg.add(at as usize + 8));
            found_n += 1;
        }
        at += rl;
        i += 1;
    }
    found_n
}

/// Validate and load the `.dkm` at `path`. See `dkrt_load`. The tables are checked after every attempt, and none starts once a check has
/// failed (`table::check`).
pub unsafe fn load(path: *const u8) -> i32 {
    if crate::table::bad() {
        return E_TABLE;
    }
    let r = load_inner(path);
    #[cfg(not(test))]
    crate::table::check();
    r
}

unsafe fn load_inner(path: *const u8) -> i32 {
    dsp::init_once();
    // the module set is fixed per session: only the boot hook loads (hot reload is archived, docs/plans/digikit-hot-reload.md)
    if !EARLY {
        return E_BOOT_ONLY;
    }
    let found = match store::find(path) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let r = store::load_to_stage(&found);
    if r != OK {
        return r;
    }
    let f = store::stage() as *const u8;
    let size = found.size;

    // ---- checks (nothing is written outside the staging buffer) ----
    if size < HEADER || rd32(f) != MAGIC || rd32(f.add(4)) != 1 {
        return E_FORMAT;
    }
    let hdr = rd32(f.add(H_HDRSIZE));
    if rd32(f.add(H_SIZE)) != size || hdr != HEADER {
        return E_FORMAT;
    }
    // the file's own sha256, over the file with the sha field read as zeros
    let mut want = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        want[i] = *f.add(H_SHA + i);
        i += 1;
    }
    let mut sha = Sha::new();
    sha.update(f, H_SHA as u32);
    sha.update_zeros(32);
    sha.update(f.add(H_SHA + 32), size - (H_SHA as u32 + 32));
    let mut got = [0u8; 32];
    sha.finish(got.as_mut_ptr());
    if got != want {
        return E_SHA;
    }
    // contracts (D3): refused only when a contract the module declares (major != 0) has another major than the runtime's, or a
    // minor above the runtime's. The runtime version in H_RUNTIME is informational.
    let rt = [
        contract::API,
        contract::MACHINE,
        contract::FX,
        contract::SERVICE,
    ];
    let mut c = 0;
    while c < 4 {
        let major = *f.add(H_CONTRACT + 2 * c);
        let minor = *f.add(H_CONTRACT + 2 * c + 1);
        if major != 0 && (major != rt[c].0 || minor > rt[c].1) {
            return E_CONTRACT;
        }
        c += 1;
    }
    let name = f.add(H_NAME);
    let nlen = cstr_len(name, 32);
    if nlen == 0 || nlen == 32 {
        return E_FORMAT;
    }
    let image_off = rd32(f.add(H_IMAGE_OFF));
    let image_size = rd32(f.add(H_IMAGE_SIZE));
    let bss = rd32(f.add(H_BSS));
    let align = rd32(f.add(H_ALIGN));
    let reloc_off = rd32(f.add(H_RELOC_OFF));
    let reloc_n = rd32(f.add(H_RELOC_N));
    let import_off = rd32(f.add(H_IMPORT_OFF));
    let import_n = rd32(f.add(H_IMPORT_N));
    let init = rd32(f.add(H_INIT));
    let fini = rd32(f.add(H_FINI));
    let menu = rd32(f.add(H_MENU));
    let tick = rd32(f.add(H_TICK));
    if align == 0
        || align > 16
        || align & (align - 1) != 0
        || image_size == 0
        || image_size > size
        || bss > crate::ARENA_BYTES as u32
    {
        return E_FORMAT;
    }
    if !in_file(image_off, image_size, size)
        || image_off & 15 != 0
        || reloc_n > dkneeds::MAX_RELOCS
        || import_n > dkneeds::MAX_CF_IMPORTS
        || !in_file(reloc_off, reloc_n * 8, size)
        || !in_file(import_off, import_n * 32, size)
    {
        return E_FORMAT;
    }
    let total = image_size + bss;
    if (init != NONE && init >= image_size)
        || (fini != NONE && fini >= image_size)
        || (menu != NONE && menu >= image_size)
        || (tick != NONE && tick >= image_size)
    {
        return E_FORMAT;
    }
    // imports: every name must be in the runtime's table
    let mut import_addrs = [0u32; 64];
    i = 0;
    while i < import_n as usize {
        let p = f.add(import_off as usize + i * 32);
        if cstr_len(p, 32) == 32 {
            return E_FORMAT;
        }
        let a = import_addr(p);
        if a == 0 {
            // the first import that belongs to a module that is not in the image: the boot keeps its name (`MISSING`)
            if let (Some(n), true) = (crate::import_needs(p), MISSING[0] == 0) {
                let b = n.as_bytes();
                let mut k = 0;
                while k < b.len() && k < NEEDS_LEN {
                    MISSING[k] = b[k];
                    k += 1;
                }
            }
            return E_IMPORT;
        }
        import_addrs[i] = a;
        i += 1;
    }
    // relocations: kind, bounds, import index
    i = 0;
    while i < reloc_n as usize {
        let e = f.add(reloc_off as usize + i * 8);
        let off = rd32(e);
        let kind = rd32(e.add(4));
        let k = kind >> 24;
        if off > image_size - 4 || (k != 0 && k != 1) || (k == 1 && (kind & 0xff_ffff) >= import_n)
        {
            return E_FORMAT;
        }
        i += 1;
    }
    let s = st();
    // `register` records (machines, parameters): validated now, written after the copy
    let (reg_off, reg_size) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
    if !in_file(reg_off, reg_size, size) {
        return E_FORMAT;
    }
    let reg = f.add(reg_off as usize);
    let r = crate::registry::check(reg, reg_size, image_size);
    if r != OK {
        return r;
    }
    // the DSP section, if any
    let (dsp_off, dsp_size) = (rd32(f.add(H_DSP_OFF)), rd32(f.add(H_DSP_SIZE)));
    let mut job = dsp::Job::empty();
    if dsp_size != 0 {
        let r = dsp::check(f, dsp_off, dsp_size, size, &mut job);
        if r != OK {
            return r;
        }
    }
    // already loaded; a free record
    let rec = match capacity::pick_record(MAX_MODULES, |i| s.modules[i].used != 0, |i| cstr_eq(s.modules[i].name.as_ptr(), name)) {
        Ok(r) => r,
        Err(e) => return e,
    };

    // ---- allocate, copy, relocate, zero bss ----
    // The DSP range, the relocated DSP image and the voice rows come first: nothing else is allocated yet if they fail.
    if dsp_size != 0 {
        let r = dsp::reserve(rec, &job);
        if r != OK {
            return r;
        }
    }
    let mem = arena::alloc(total);
    if mem.is_null() {
        if dsp_size != 0 {
            dsp::cancel(rec);
        }
        return E_NOMEM;
    }
    let base = mem as u32;
    i = 0;
    while i < image_size as usize {
        *mem.add(i) = *f.add(image_off as usize + i);
        i += 1;
    }
    i = 0;
    while i < bss as usize {
        *mem.add(image_size as usize + i) = 0;
        i += 1;
    }
    i = 0;
    while i < reloc_n as usize {
        let e = f.add(reloc_off as usize + i * 8);
        let off = rd32(e) as usize;
        let kind = rd32(e.add(4));
        let add = if kind >> 24 == 0 {
            base
        } else {
            import_addrs[(kind & 0xff_ffff) as usize]
        };
        let p = mem.add(off);
        wr32(p, rd32(p).wrapping_add(add));
        i += 1;
    }
    // ---- caches: write the lines back and drop them from both caches ----
    flush(base, total);

    // ---- record, init, publish ----
    let m: &mut Module = &mut s.modules[rec];
    m.used = 1;
    i = 0;
    while i < 32 {
        m.name[i] = *name.add(i);
        i += 1;
    }
    let dst = &mut (*addr_of_mut!(crate::DKRT_PATHS))[rec];
    let plen = cstr_len(path, 63);
    i = 0;
    while i < 64 {
        dst[i] = if i < plen { *path.add(i) } else { 0 };
        i += 1;
    }
    m.version = rd32(f.add(H_VERSION));
    let ct = &mut (*addr_of_mut!(crate::DKRT_CONTRACTS))[rec];
    i = 0;
    while i < 8 {
        ct[i] = *f.add(H_CONTRACT + i);
        i += 1;
    }
    m.base = base;
    m.size = total;
    m.fini = if fini == NONE { 0 } else { base + fini };
    crate::entry::bind(rec);
    (*addr_of_mut!(crate::DKRT_MENUS))[rec] = if menu == NONE { 0 } else { base + menu };
    let initaddr = if init == NONE { 0 } else { base + init };
    let tickaddr = if tick == NONE { 0 } else { base + tick };
    if dsp_size != 0 {
        // the CF part goes live now (rows in place before the machine view is built); the DSP code and the voice rows follow
        // when the frames flow
        m.used = 1;
        let r = complete(rec, reg, reg_size, initaddr, tickaddr);
        if r != OK {
            if dsp::retained() { return E_BUSY; }
            dsp::cancel(rec);
            if !crate::files::retire(rec) || !retire_sdk(rec) { return E_BUSY; }
            arena::free(mem);
            m.used = 0;
            crate::entry::unbind(rec);
            return r;
        }
        dsp::launch(dsp::Pending {
            rec: rec as u32,
            reg: 0,
            reg_size: 0,
            init: 0,
            early: 1,
            boot_idx: BOOT_IDX,
            send_base: 0,
            send_len: 0,
            send_buf: 0,
        });
        return LOADING;
    }
    let r = complete(rec, reg, reg_size, initaddr, tickaddr);
    if r != OK {
        if dsp::retained() || !crate::files::retire(rec) || !retire_sdk(rec) { return E_BUSY; }
        arena::free(mem);
        m.used = 0;
        crate::entry::unbind(rec);
    }
    r
}

/// The end of a load, shared by the immediate and the DSP path (both run in the boot hook): the registry rows (the machine is listed from here), `init`, the
/// tick. On an error everything it did is undone (the DSP part too) and the caller frees the module's memory.
unsafe fn complete(rec: usize, reg: *const u8, reg_size: u32, init: u32, tick: u32) -> i32 {
    let s = st();
    let owner = rec as u8 + 1;
    crate::files::init_begin(rec);
    // registry rows before `init` (which may reuse the staging buffer through `store_read`)
    let r = crate::registry::apply(owner, reg, reg_size, s.modules[rec].base);
    if r != OK {
        crate::registry::remove(owner);
        let held = !crate::files::retire(rec) | !retire_sdk(rec);
        if !dsp::release(rec) || held { return E_BUSY; }
        return r;
    }
    if init != 0 {
        let call: unsafe extern "C" fn(*const Ctx) -> i32 = core::mem::transmute(init as usize);
        let ctx = crate::entry::ctx_addr(rec) as usize as *const Ctx;
        CALLED_NEED = [0; NEEDS_LEN];
        let r = crate::dlog::call_init(rec as usize, call, ctx);
        if r != 0 {
            // the last service accessor whose provider is not in the image answered DK_E_MISSING: the boot shows the provider as the module's need
            let missing = r == E_MISSING && CALLED_NEED[0] != 0;
            if missing && MISSING[0] == 0 {
                MISSING = CALLED_NEED;
            }
            crate::intro::discard(rec);
            let held = !crate::files::retire(rec) | !retire_sdk(rec);
            crate::upload::release_cmds(owner);
            crate::registry::remove(owner);
            if !dsp::release(rec) || held { return E_BUSY; }
            // the load fails as for an unresolved import of a module that is not in the image (the LOADS and BOOT views know that code)
            return if missing { E_IMPORT } else { E_INIT };
        }
    }
    if tick != 0 {
        crate::entry::publish_tick(rec, tick);
    }
    let m: &mut Module = &mut s.modules[rec];
    m.used = 1;
    s.loads = s.loads.wrapping_add(1);
    crate::files::ready(rec);
    // fully loaded: a boot animation the module asked for in `init` goes live now (never before: a rolled-back module is not reachable)
    crate::intro::commit(rec);
    OK
}

/// The DSP part of a module published at boot failed (the machine rows and `init` are in already): take it down again.
pub unsafe fn undo_early(rec: usize) -> i32 {
    unload_rec(rec)
}

/// While the boot hook loads the selection: the CF part of a module with DSP code is published at once (the machine view is built
/// soon after the hook, and the frames that carry DSP code do not flow yet); the DSP part follows over the UI-loop passes.
pub static mut EARLY: bool = false;
/// The name of the missing module of the last load that failed with E_IMPORT (`import_needs`), NUL padded; empty when none. The boot clears it
/// before each load and takes it after.
pub static mut MISSING: [u8; NEEDS_LEN] = [0; NEEDS_LEN];
/// The provider of the last `_open` stub that answered DK_E_MISSING since the clear before `init` (`stub_missing`), NUL padded; empty when none.
/// When `init` returns DK_E_MISSING, `MISSING` takes it and the load fails with E_IMPORT, as for an unresolved import.
static mut CALLED_NEED: [u8; NEEDS_LEN] = [0; NEEDS_LEN];

/// Called by the stub of an absent service's `_open`: records `provider` (`core/usb-bulk` becomes `usb-bulk`) as the need, replacing an earlier one.
pub fn stub_missing(provider: &'static str) {
    let n = match provider.rfind('/') {
        Some(at) => &provider[at + 1..],
        None => provider,
    };
    let b = n.as_bytes();
    unsafe {
        CALLED_NEED = [0; NEEDS_LEN];
        let mut k = 0;
        while k < b.len() && k < NEEDS_LEN {
            CALLED_NEED[k] = b[k];
            k += 1;
        }
    }
}
/// The selection index of the module being loaded by the boot hook (`u32::MAX`: none).
pub static mut BOOT_IDX: u32 = u32::MAX;

extern "C" {
    /// `core/dsp-link`: clears the per-block callback when it points into [lo, hi). Resolves to 0 when the module is not in the image.
    fn dl_tick_cb_drop();
}

/// After a module's `fini`: no per-block callback of `core/dsp-link` may point into its memory (a module that forgot to clear it).
unsafe fn drop_tick_cb(base: u32, size: u32) {
    let a = core::hint::black_box(dl_tick_cb_drop as unsafe extern "C" fn() as usize);
    if a != 0 {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(a);
        f(base, base.wrapping_add(size));
    }
}

/// Take module record `rec` down: its tick, `fini`, machine rows, DSP voice rows and range, memory. Used only
/// by the DSP loader when the DSP part of a module published at boot fails (`undo_early`). The run-time `unload` that also called
/// it (with the in-use scan) is archived with hot reload. The tables are checked after it (`table::check`).
pub unsafe fn unload_rec(rec: usize) -> i32 {
    let r = unload_rec_inner(rec);
    #[cfg(not(test))]
    crate::table::check();
    r
}

/// The module `rec` is going away (an unload or a rollback): its I/O jobs end without `done`, and a file it was writing is dropped. False: an
/// I/O step of it is running (it is cancelled); the caller keeps the module's memory (`E_BUSY`) until a later call returns true.
pub(crate) unsafe fn retire_sdk(rec: usize) -> bool {
    if !crate::io::retire(rec) {
        return false;
    }
    crate::store_writer::retire(rec as u8 + 1);
    true
}

unsafe fn unload_rec_inner(rec: usize) -> i32 {
    if !crate::files::retire(rec) { return E_BUSY; }
    if !crate::worker::retire(rec) { return E_BUSY; }
    if !retire_sdk(rec) { return E_BUSY; }
    // an intro of this module that has not run `done` yet: `done` must not point into freed code
    crate::intro::discard(rec);
    let s = st();
    let m: &mut Module = &mut s.modules[rec];
    // 1. clear the tick (one aligned store) so no new call starts; the UI task, where it runs, is this task
    crate::entry::stop_tick(rec);
    // DSP rows must be invalidated before fini can mutate/release module state.
    if !dsp::quiesce(rec) { return E_BUSY; }
    // 2. fini, 3. free
    if m.fini != 0 {
        #[cfg(test)] crate::cleanup_tests::fini(rec, m.fini);
        #[cfg(not(test))] {
            let call: unsafe extern "C" fn(*const Ctx) = core::mem::transmute(m.fini as usize);
            let ctx = crate::entry::ctx_addr(rec) as usize as *const Ctx;
            crate::dlog::call_fini(rec as usize, call, ctx);
        }
    }
    // the SysEx commands it registered are released with it
    crate::upload::release_cmds(rec as u8 + 1);
    drop_tick_cb(m.base, m.size);
    // the machine row first, then the DSP voice rows, then the range in the DSP area
    #[cfg(test)] crate::cleanup_tests::registry_remove(rec as u8 + 1);
    #[cfg(not(test))] crate::registry::remove(rec as u8 + 1);
    dsp::release_range(rec);
    arena::free(m.base as *mut u8);
    m.used = 0;
    (*addr_of_mut!(crate::DKRT_MENUS))[rec] = 0;
    crate::entry::unbind(rec);
    OK
}
