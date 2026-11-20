//! The builder call of the shortcuts service, `dk_shortcut_add`, and its commit. The call collects the module's shortcuts while its `init` runs (`module_begin` opens the
//! collection for module record `rec`); `module_commit` checks them and writes them as the `register` section's SHORTCUT records (`PREPARE` checks and measures, `WRITE`
//! writes), with the callbacks as offsets into the module image (an address of the loaded module is not valid after a reload). The loader's registry walker then calls
//! this service's `check` and `apply_record`.
//!
//! One module is in `init` at a time, so the collection is one static. A call outside `init`, with another module's context or with a bad argument returns an error code
//! and the commit refuses the module with the first error.
use crate::bindings::{Callback, ID_BYTES, LABEL_BYTES};
use crate::validate::{at, BODY_LEN, MAX_PER_MODULE};
use core::ffi::c_void;
use core::ptr::{addr_of_mut, read_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::{commit_phase, ModuleCommitCtx};
use dkrt_sdk::records::{KIND_SHORTCUT, LEN_SHORTCUT};
use dkrt_sdk::*;
use dkrt_svc_params::builder::{fail, Gate};

/// The struct a module passes to `dk_shortcut_add` (`dk_shortcut_def` of <dk/platform/shortcuts.h>): the same layout on the ColdFire.
#[repr(C)]
pub struct ShortcutDef {
    pub id: *const u8,
    pub label: *const u8,
    pub keys: [u32; 2],
    pub on: Option<Callback>,
    pub off: Option<Callback>,
    pub arg: *mut c_void,
}

/// One collected shortcut: texts copied, the callbacks as the addresses the module gave.
#[derive(Clone, Copy)]
struct Item {
    id: [u8; ID_BYTES],
    label: [u8; LABEL_BYTES],
    keys: [u32; 2],
    on: usize,
    off: usize,
    arg: usize,
}

const ITEM: Item = Item {
    id: [0; ID_BYTES],
    label: [0; LABEL_BYTES],
    keys: [0; 2],
    on: 0,
    off: 0,
    arg: 0,
};

/// The open collection. All zero bytes are the empty one; `begin` writes it before the first call of every module.
pub struct Collection {
    n: usize,
    err: i32,
    items: [Item; MAX_PER_MODULE],
}

static mut COLLECTION: Collection = Collection {
    n: 0,
    err: 0,
    items: [ITEM; MAX_PER_MODULE],
};
static mut GATE: Gate = Gate::new();

fn gate() -> &'static mut Gate {
    unsafe { &mut *addr_of_mut!(GATE) }
}

fn collection() -> &'static mut Collection {
    unsafe { &mut *addr_of_mut!(COLLECTION) }
}

/// `module_begin`: forget what an earlier module collected and open the collection for module record `rec`.
pub fn begin(rec: usize) {
    *collection() = Collection {
        n: 0,
        err: 0,
        items: [ITEM; MAX_PER_MODULE],
    };
    gate().begin(rec);
}

/// Close the collection: later calls are refused.
pub fn end() {
    gate().end();
}

/// The call `dk_shortcut_add(ctx, def)`: returns 0, or the error that the commit will report.
///
/// # Safety
/// `ctx` is any pointer (the core only compares it); `def` is null or a `dk_shortcut_def` whose `id` and `label` are null or NUL-terminated.
pub unsafe fn shortcut_add<C: CoreApi>(core: &C, ctx: *mut u8, def: *const u8) -> i32 {
    if !gate().open_for(core, ctx) {
        return E_CONTEXT;
    }
    let c = collection();
    if def.is_null() {
        fail(&mut c.err, E_FORMAT);
        return E_FORMAT;
    }
    if c.n >= MAX_PER_MODULE {
        fail(&mut c.err, E_DECL_FULL);
        return E_DECL_FULL;
    }
    let d = &*(def as *const ShortcutDef);
    let mut err = 0;
    let mut it = ITEM;
    put(&mut it.id, d.id, &mut err);
    put(&mut it.label, d.label, &mut err);
    it.keys = read_volatile(&d.keys);
    it.on = d.on.map_or(0, |f| f as usize);
    it.off = d.off.map_or(0, |f| f as usize);
    it.arg = d.arg as usize;
    if err == 0 {
        // the keys, the callback and the texts are judged by the commit's rule (`validate`), on the record bytes: one rule for the load and for `module:check`
        c.items[c.n] = it;
        c.n += 1;
        return OK;
    }
    fail(&mut c.err, err);
    err
}

/// The C string `src` into the NUL padded field `dst`: null, empty or without room for the NUL is `E_FORMAT`.
unsafe fn put(dst: &mut [u8], src: *const u8, err: &mut i32) {
    dkrt_svc_params::builder::put_text(dst, src, err);
}

fn be(buf: &mut [u8; BODY_LEN], off: usize, v: u32) {
    let b = v.to_be_bytes();
    let mut i = 0;
    while i < 4 {
        buf[off + i] = b[i];
        i += 1;
    }
}

/// The body of record `i` of the collection (`at::*` layout); the callbacks are offsets from `base`, 0 when absent.
fn body(it: &Item, base: u32) -> [u8; BODY_LEN] {
    let mut b = [0u8; BODY_LEN];
    b[at::ID..at::ID + ID_BYTES].copy_from_slice(&it.id);
    b[at::LABEL..at::LABEL + LABEL_BYTES].copy_from_slice(&it.label);
    be(&mut b, at::KEY0, it.keys[0]);
    be(&mut b, at::KEY1, it.keys[1]);
    // offset 0 means no callback: it is the module header, never code
    let off = |a: usize| {
        if a == 0 {
            0
        } else {
            (a as u32).wrapping_sub(base)
        }
    };
    be(&mut b, at::ON, off(it.on));
    be(&mut b, at::OFF, off(it.off));
    be(&mut b, at::ARG, it.arg as u32);
    b
}

/// The commit of module record `rec` (`Service::module_commit`): `PREPARE` checks the collection (the first error of a call, the rule of `validate` on the records, the
/// callbacks inside the image) and measures, `WRITE` writes the records, `DECLARE` closes the collection.
///
/// # Safety
/// `c.sink` is the loader's sink.
pub unsafe fn commit<C: CoreApi>(_core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
    if !gate().is_open_for(rec) {
        return OK; // no `module_begin` for this record: nothing was collected
    }
    let col = collection();
    if c.phase == commit_phase::DECLARE || (col.n == 0 && col.err == 0) {
        end();
        return OK;
    }
    if col.err != 0 {
        return col.err;
    }
    let sink = &mut *c.sink;
    let mut bodies = [[0u8; BODY_LEN]; MAX_PER_MODULE];
    let mut i = 0;
    while i < col.n {
        bodies[i] = body(&col.items[i], c.base);
        i += 1;
    }
    if c.phase == commit_phase::PREPARE {
        let mut refs: [&[u8]; MAX_PER_MODULE] = [&[]; MAX_PER_MODULE];
        let mut k = 0;
        while k < col.n {
            refs[k] = &bodies[k];
            k += 1;
        }
        let mut first = OK;
        crate::validate::check_set(&refs[..col.n], &mut |p| {
            if first == OK {
                first = p.code;
            }
        });
        if first != OK {
            return first;
        }
        // the callbacks are code of the module's image
        let mut k = 0;
        while k < col.n {
            let (on, off) = (rd(&bodies[k], at::ON), rd(&bodies[k], at::OFF));
            if on >= c.image_size || off >= c.image_size {
                return E_FORMAT;
            }
            k += 1;
        }
    }
    let mut k = 0;
    while k < col.n {
        sink.head(KIND_SHORTCUT, LEN_SHORTCUT as usize);
        sink.bytes(&bodies[k]);
        k += 1;
    }
    OK
}

fn rd(b: &[u8; BODY_LEN], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
