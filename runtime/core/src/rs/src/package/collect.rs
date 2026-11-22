//! The generic part of a module's `init`: the records it adds with `dk_record_add` and the declarations of a service file (`dk_service_declare_*`), and the module's header
//! table against them (declare-check-commit). The builder calls that name machines and effects are the exports of the services that own them (`machines.dks`, `fx.dks`);
//! each service keeps its own collection and writes its records at the commit (`Service::module_commit`).
//!
//! During `init` every `dk_record_add` and `dk_service_declare_*` call appends to a `Collect`, one block in the module's arena share (`begin` .. `end`). A call never fails one
//! by one: on overflow, on a bad argument or outside `init` it returns an error and the first error is kept. The commit (`loader::commit`) reports it, compares the declarations
//! with the header table (`decl_matches`) and appends the generic records to the block the services write. The record block stays in the arena while the module is loaded
//! (`BLOCKS`); the `MODULE_COMMIT` event carries its address and size.
use dkrt_abi::MAX_MODULES;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::service::{DeclItem, Declare, RecordSink};
use dkrt_sdk::*;

/// The most records of a module and the bytes of the generic records' bodies one `init` may emit.
pub const MAX_RECORDS: usize = dkneeds::MAX_RECORDS as usize;
/// The most generic records (`dk_record_add`) and the bytes of their bodies one `init` may emit.
pub const MAX_G: usize = 8;
pub const G_BYTES: usize = 640;

/// A generic record: its kind and where its body is in `Collect::gbytes`.
#[derive(Clone, Copy)]
pub struct GRec {
    pub kind: u32,
    pub at: u16,
    pub len: u16,
}

/// What `init` declared.
pub struct Collect {
    pub err: i32,
    /// the generic records (`dk_record_add`): kind and body, appended to the `register` section as they came
    pub grecs: [GRec; MAX_G],
    pub ng: usize,
    /// what a service module declared in `init` (`dk_service_declare_*`), compared with its header table at the commit
    pub decl: Declare,
    gbytes: [u8; G_BYTES],
    gat: usize,
}

// The host tests' tables (`COLLECT_BYTES`, sdk/sdk.toml `[limits.format]`) are for the ColdFire layout; the host's 64-bit pointers pad it more.
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<Collect>() == dkneeds::COLLECT_BYTES as usize);

impl Collect {
    pub const fn new() -> Collect {
        Collect { err: 0, grecs: [GRec { kind: 0, at: 0, len: 0 }; MAX_G], ng: 0, decl: Declare::new(), gbytes: [0; G_BYTES], gat: 0 }
    }

    /// `dk_record_add`: keep one record of a generic kind (`[records.kinds]` `generic = true`; the typed kinds have their builders). The body is
    /// copied; its length is the kind's record length less the 8-byte header. The owning service checks the record at the commit. Returns OK
    /// or the error that the commit reports too.
    pub fn add_record(&mut self, kind: u32, bytes: *const u8, len: u32) -> i32 {
        let want = match crate::package::handlers::len(kind) {
            Some(l) if is_generic(kind) => l,
            _ => {
                self.fail(E_FORMAT);
                return E_FORMAT;
            }
        };
        if bytes.is_null() || len + 8 != want {
            self.fail(E_FORMAT);
            return E_FORMAT;
        }
        if self.ng >= MAX_G || self.gat + len as usize > G_BYTES {
            self.fail(E_DECL_FULL);
            return E_DECL_FULL;
        }
        let mut i = 0;
        while i < len as usize {
            self.gbytes[self.gat + i] = unsafe { *bytes.add(i) };
            i += 1;
        }
        self.grecs[self.ng] = GRec { kind, at: self.gat as u16, len: len as u16 };
        self.ng += 1;
        self.gat += len as usize;
        OK
    }

    fn fail(&mut self, e: i32) {
        if self.err == 0 {
            self.err = e;
        }
    }

    /// Append the generic records to `o`, as they were added: the owning service checks and applies them.
    pub fn write_generic(&self, o: &mut RecordSink) {
        let mut g = 0;
        while g < self.ng {
            let r = self.grecs[g];
            o.head(r.kind, 8 + r.len as usize);
            o.bytes(&self.gbytes[r.at as usize..r.at as usize + r.len as usize]);
            g += 1;
        }
    }
}

// ---- the active collection --------------------------------------------------------------------------------------------------

static mut ACTIVE: *mut Collect = core::ptr::null_mut();

/// Start collecting into the block `c` (initialised here); the builder calls reach it until `end`.
pub unsafe fn begin(c: *mut Collect) {
    core::ptr::write(c, Collect::new());
    write_volatile(addr_of_mut!(ACTIVE), c);
}

/// Stop collecting: later `dk_record_add` and `dk_service_declare_*` calls fail.
pub unsafe fn end() {
    write_volatile(addr_of_mut!(ACTIVE), core::ptr::null_mut());
}

unsafe fn active() -> Option<&'static mut Collect> {
    let c = read_volatile(addr_of!(ACTIVE));
    if c.is_null() {
        None
    } else {
        Some(&mut *c)
    }
}


pub unsafe extern "C" fn dk_record_add(ctx: *mut u8, kind: u32, bytes: *const u8, len: u32) -> i32 {
    match active() {
        Some(c) if crate::package::entry::is_ctx(ctx) => c.add_record(kind, bytes, len),
        _ => E_CONTEXT,
    }
}
/// One `dk_service_declare_*` call: the item goes to the declarations, which have their own bound (`Declare::MAX`) apart from the records of the builder calls.
unsafe fn declare(ctx: *mut u8, tag: u16, a: u32, b: u32, c: u32) -> i32 {
    match active() {
        Some(col) if crate::package::entry::is_ctx(ctx) => {
            if col.decl.n >= Declare::MAX {
                col.fail(E_DECL_FULL);
                return E_DECL_FULL;
            }
            col.decl.add(tag, a, b, c);
            OK
        }
        _ => E_CONTEXT,
    }
}
/// `dk_service_declare_row` (a service file's `init`, `SVC_IMPORTS` only): the `ServiceRow` the file provides. One row per file; the commit hands it to the
/// registry (`Registry::add`).
#[cfg_attr(test, no_mangle)]
pub unsafe extern "C" fn dk_service_declare_row(ctx: *mut u8, row: *const dkrt_abi::service::ServiceRow) -> i32 {
    match active() {
        Some(col) if crate::package::entry::is_ctx(ctx) => {
            if row.is_null() || col.decl.row_ptr().is_some() {
                col.fail(E_DECL_KIND);
                return E_DECL_KIND;
            }
            if col.decl.n >= Declare::MAX {
                col.fail(E_DECL_FULL);
                return E_DECL_FULL;
            }
            col.decl.row(row);
            OK
        }
        _ => E_CONTEXT,
    }
}
#[cfg_attr(test, no_mangle)]
pub unsafe extern "C" fn dk_service_declare_kind(ctx: *mut u8, kind: u32, len: u32, handler: *const u8) -> i32 {
    // a handler is an image address: the header table carries its offset in the image
    declare(ctx, Declare::KIND, kind, len, DevAddr::of_abi_ptr(handler).get())
}
#[cfg_attr(test, no_mangle)]
pub unsafe extern "C" fn dk_service_declare_slot(ctx: *mut u8, slot: u32, flags: u32, entry: *const u8) -> i32 {
    // the entry is an image address: the header table carries its offset in the image
    declare(ctx, Declare::SLOT, slot, flags, DevAddr::of_abi_ptr(entry).get())
}
#[cfg_attr(test, no_mangle)]
pub unsafe extern "C" fn dk_service_declare_hook(ctx: *mut u8, site: u32, entry: *const u8) -> i32 {
    declare(ctx, Declare::HOOK, site, DevAddr::of_abi_ptr(entry).get(), 0)
}
#[cfg_attr(test, no_mangle)]
pub unsafe extern "C" fn dk_service_declare_export(ctx: *mut u8, name_hash: u32, entry: *const u8) -> i32 {
    declare(ctx, Declare::EXPORT, name_hash, DevAddr::of_abi_ptr(entry).get(), 0)
}
#[cfg_attr(test, no_mangle)]
pub unsafe extern "C" fn dk_service_declare_resource(ctx: *mut u8, resource: u32, limit: u32) -> i32 {
    declare(ctx, Declare::RESOURCE, resource, limit, 0)
}

// ---- the keys table of the header --------------------------------------------------------------------------------------------

pub use dkrt_abi::keys::KeysCopy;

/// Whether `kind` is a generic kind: a record a module emits with `dk_record_add` (`[records.kinds]` `generic = true`).
fn is_generic(kind: u32) -> bool {
    let mut i = 0;
    while i < dkrt_sdk::records::GENERIC_KINDS.len() {
        if dkrt_sdk::records::GENERIC_KINDS[i] == kind {
            return true;
        }
        i += 1;
    }
    false
}

/// What `init` declared (`dk_service_declare_*`, addresses in the loaded image at `base`) with the entries that are addresses made image offsets, as the header table
/// has them.
fn declared_offsets(c: &Collect, base: u32) -> Declare {
    let mut d = c.decl;
    let mut i = 0;
    while i < d.n {
        let DeclItem { tag, a, b, c: e } = d.items[i];
        // the entries that are addresses become image offsets, as the table has them
        d.items[i] = if tag == Declare::KIND {
            DeclItem { tag, a, b, c: e.wrapping_sub(base) }
        } else if tag == Declare::SLOT {
            DeclItem { tag, a, b, c: e.wrapping_sub(base) }
        } else if tag == Declare::HOOK || tag == Declare::EXPORT {
            DeclItem { tag, a, b: b.wrapping_sub(base), c: e }
        } else {
            DeclItem { tag, a, b, c: e }
        };
        i += 1;
    }
    d
}

/// Whether what `init` declared (`dk_service_declare_*`, addresses in the loaded image at `base`) is what the header table carries (declare-check-commit):
/// the same kinds with the same lengths and handlers, slots, hooks, exports and resources. A module that declares nothing and has no such entries passes.
pub fn decl_matches(c: &Collect, keys: &KeysCopy, base: u32) -> bool {
    declared_offsets(c, base).same_as(&keys.decl)
}

/// The first item that `a` and `b` do not both hold (the `ROW` item of `a` is left out, as in `Declare::same_as`): the item, and whether it is `a`'s (else it is `b`'s).
fn first_unmatched(a: &Declare, b: &Declare) -> Option<(DeclItem, bool)> {
    let has = |d: &Declare, x: DeclItem| {
        let mut k = 0;
        while k < d.n {
            if d.items[k] == x {
                return true;
            }
            k += 1;
        }
        false
    };
    let mut i = 0;
    while i < a.n {
        if a.items[i].tag != Declare::ROW && !has(b, a.items[i]) {
            return Some((a.items[i], true));
        }
        i += 1;
    }
    i = 0;
    while i < b.n {
        if !has(a, b.items[i]) {
            return Some((b.items[i], false));
        }
        i += 1;
    }
    None
}

/// `None` when `decl_matches`; else the first item that `init` and the header table do not both hold: (its tag, its first word), with 0x100 added to the tag when only
/// the header table has it. A difference without such an item (a full collection) is `(0, 0)` with a tag of 1.
pub fn decl_diff(c: &Collect, keys: &KeysCopy, base: u32) -> Option<(u32, u32)> {
    let d = declared_offsets(c, base);
    if d.same_as(&keys.decl) {
        return None;
    }
    Some(match first_unmatched(&d, &keys.decl) {
        Some((item, declared)) => (item.tag as u32 | if declared { 0 } else { 0x100 }, item.a),
        None => (1, 0),
    })
}

/// A module binds its DSP section to the machines and effects of its keys table: each voice record names a machine key of the
/// table (`voice_key(i)`), and the section has one effect record for each effect id of the table, in the table's order (`fx_ordinal`).
pub fn check_dsp_keys(keys: &KeysCopy, nvoice: usize, voice_key: impl Fn(usize) -> u32, nfx: usize) -> i32 {
    let mut i = 0;
    while i < nvoice {
        if !contains(&keys.mk[..keys.nm], voice_key(i)) {
            return E_DSP_TYPE;
        }
        i += 1;
    }
    if nfx != keys.nf {
        return E_DECL_ID;
    }
    OK
}

fn contains(a: &[u32], v: u32) -> bool {
    let mut i = 0;
    while i < a.len() {
        if a[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

// ---- what a loaded module keeps ---------------------------------------------------------------------------------------------

/// Per module record: the arena block of the committed record block (address, size; 0: none).
static mut BLOCKS: [(u32, u32); MAX_MODULES] = [(0, 0); MAX_MODULES];

/// Remember the committed block.
pub unsafe fn keep(rec: usize, addr: u32, size: u32) {
    (*addr_of_mut!(BLOCKS))[rec] = (addr, size);
}

/// The module `rec` goes away: forget its record block. Returns the block's address (0: none) for the caller to free.
pub unsafe fn release(rec: usize) -> u32 {
    let a = (*addr_of!(BLOCKS))[rec].0;
    (*addr_of_mut!(BLOCKS))[rec] = (0, 0);
    a
}

/// The committed block of module `rec`: (address, size), (0, 0) when it has none.
pub fn block_of(rec: usize) -> (u32, u32) {
    unsafe { (*addr_of!(BLOCKS))[rec] }
}

#[cfg(test)]
mod tests;
