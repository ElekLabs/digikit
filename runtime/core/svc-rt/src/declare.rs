//! What a service file declares to the runtime in `init`: its record kinds, its row, its hook sites, slot fills and exports (the `dk_service_declare_*` imports of
//! `sdk.toml`), and the module context `init` was called with.
use crate::import_core::sys::dk_service_declare_row;
use crate::import_core::ImportCore;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::ServiceRow;

extern "C" {
    /// An import of every module (`dk_service_declare_kind` of sdk.toml): one record kind this service owns, with its length and handler entry.
    fn dk_service_declare_kind(ctx: *mut u8, kind: u32, len: u32, handler: *const u8) -> i32;
}
const _: dkrt_sdk::api::DkServiceDeclareKindFn = dk_service_declare_kind;
extern "C" {
    /// An import of every module (`dk_service_declare_hook` of sdk.toml): one hook site this service takes (the `name_hash` of the hook's name) and the function that
    /// runs there. The runtime compares it with the `HOOKS` of the header table at the commit and installs the site after `init`.
    fn dk_service_declare_hook(ctx: *mut u8, site: u32, entry: *const u8) -> i32;
    /// An import of every module (`dk_service_declare_slot` of sdk.toml): one pointer slot of an image adapter this service fills (the `name_hash` of the slot's name) and the
    /// function whose address the installer writes in it. The runtime compares it with the `SLOTS` of the header table.
    fn dk_service_declare_slot(ctx: *mut u8, slot: u32, flags: u32, entry: *const u8) -> i32;
    /// An import of every module (`dk_service_declare_export` of sdk.toml): one function this service offers to other modules (the `name_hash` of its name) and its entry.
    /// The runtime compares it with the `EXPORTS` of the header table.
    fn dk_service_declare_export(ctx: *mut u8, name_hash: u32, entry: *const u8) -> i32;
}
const _: dkrt_sdk::api::DkServiceDeclareHookFn = dk_service_declare_hook;
const _: dkrt_sdk::api::DkServiceDeclareSlotFn = dk_service_declare_slot;
const _: dkrt_sdk::api::DkServiceDeclareExportFn = dk_service_declare_export;

/// `name_hash` of a name (FNV-1a 32, as `dkneeds::keys::name_hash`): the identity of a hook site in `HOOKS`.
pub const fn name_hash(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// The `[digikit] id` of a device in the form `service!` takes for the device filter of a hook (`dt2`, `dn2`; no filter: 0, every device).
#[doc(hidden)]
pub const DEVICE_DT2: u32 = dkrt_abi::profiles::DT2.id;
#[doc(hidden)]
pub const DEVICE_DN2: u32 = dkrt_abi::profiles::DN2.id;

/// The `init` of `service!`: declare each record kind the row owns (its handler entry is `handler`, an address of the file, which the header table records as
/// an image offset) and then the row. The first refusal is the result.
///
/// # Safety
/// `ctx` is the module context `init` was called with; `row` stays valid as long as the file is loaded.
pub unsafe fn init(ctx: *mut u8, row: &'static ServiceRow, handler: *const u8) -> i32 {
    for &(kind, len) in row.kinds() {
        let r = dk_service_declare_kind(ctx, kind, len, handler);
        if r != 0 {
            return r;
        }
    }
    dk_service_declare_row(ctx, row)
}

/// The declarations of `service!` after `init` (kinds and row): one `dk_service_declare_hook` for each `(site hash, entry, device)` that is for `device` (0: every
/// device), one `dk_service_declare_slot` for each `(slot hash, entry, device)` of `fills` likewise, then one `dk_service_declare_export` for each `(name hash,
/// entry)`. The first refusal is the result. `device` is the `[digikit] id` of the running device.
///
/// # Safety
/// As `init`; each entry is the address of a function of the file.
pub unsafe fn declare(
    ctx: *mut u8,
    row: &'static ServiceRow,
    handler: *const u8,
    device: fn() -> u32,
    hooks: &[(u32, *const u8, u32)],
    fills: &[(u32, *const u8, u32)],
    exports: &[(u32, *const u8)],
) -> i32 {
    let r = init(ctx, row, handler);
    if r != 0 {
        return r;
    }
    // the device is asked for only when an entry has a filter to compare with
    let device = if hooks.is_empty() && fills.is_empty() {
        0
    } else {
        device()
    };
    for &(site, entry, only) in hooks {
        if only != 0 && only != device {
            continue;
        }
        let r = dk_service_declare_hook(ctx, site, entry);
        if r != 0 {
            return r;
        }
    }
    for &(slot, entry, only) in fills {
        if only != 0 && only != device {
            continue;
        }
        let r = dk_service_declare_slot(ctx, slot, 0, entry);
        if r != 0 {
            return r;
        }
    }
    for &(name, entry) in exports {
        let r = dk_service_declare_export(ctx, name, entry);
        if r != 0 {
            return r;
        }
    }
    0
}

/// The `[digikit] id` of the running device (`dkc_device_id`): what `service!` passes to `declare`.
#[doc(hidden)]
pub fn running_device() -> u32 {
    ImportCore.device_id()
}

static mut INIT_CTX: *mut u8 = core::ptr::null_mut();

/// The module context `init` of this file was called with (`service!` stores it before `setup` runs): what a `setup` needs for the opener imports that take it (`dk_files_open`).
/// A file is one service, so there is one. Null before `init`. A host build that links several service files shares it: only the last `init` counts.
pub fn init_ctx() -> *mut u8 {
    // SAFETY: a plain word, written once in `init` (the UI task) and read after it.
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(INIT_CTX)) }
}

#[doc(hidden)]
pub fn set_init_ctx(ctx: *mut u8) {
    // SAFETY: as `init_ctx`.
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(INIT_CTX), ctx) }
}

/// Declare the optional resident query entry after the service's normal exports.
#[doc(hidden)]
pub unsafe fn declare_resident_query(ctx: *mut u8, entry: dkrt_sdk::ResidentQueryV1) -> i32 {
    dk_service_declare_export(
        ctx,
        name_hash(dkrt_sdk::RESIDENT_QUERY_ENTRY),
        entry as *const u8,
    )
}
