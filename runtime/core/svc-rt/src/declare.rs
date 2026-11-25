//! What a service file declares to the runtime in `init`: its record kinds, its row, its hook sites, slot fills and exports (the `dk_service_declare_*` imports of
//! `sdk.toml`), and the module context `init` was called with.
use crate::import_core::sys::{dk_provider_declare_row, dk_service_declare_patch};
use crate::import_core::ImportCore;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::ProviderRow;

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

pub use dkrt_abi::provider::name_hash;

/// The bytes a patch writes: a byte, a halfword or a long word (the length of the patch site in the port file), as the value `start` computed. `service!` makes one for each `patches`
/// entry; the runtime writes it at the site on load and the stock bytes back before `stop`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patch {
    pub value: u32,
    pub len: u32,
}

impl Patch {
    /// `len` bytes (1, 2 or 4) of `value`, big-endian, the low bytes of it.
    pub const fn new(value: u32, len: u32) -> Patch {
        Patch { value, len }
    }
}

/// The `[digikit] id` of a device in the form `service!` takes for the device filter of a hook (`dt2`, `dn2`; no filter: 0, every device).
#[doc(hidden)]
pub const DEVICE_DT2: u32 = dkrt_abi::profiles::DT2.id;
#[doc(hidden)]
pub const DEVICE_DN2: u32 = dkrt_abi::profiles::DN2.id;

/// The declarations of a service file (`service!`): each record kind in `kinds`, the row with the instance `Provider::start` made, then the hooks, fills, patches and exports. `instance` is null before `start` (the host test of the assembly replays the
/// declarations without one; the runtime refuses such a row at the commit).
///
/// # Safety
/// `ctx` is the module context `init` was called with; each entry is the address of a function of the file; `row` stays valid as long as the file is loaded and `instance` as long as the row is.
#[allow(clippy::too_many_arguments)]
pub unsafe fn declare_provider(
    ctx: *mut u8,
    kinds: &'static [(u32, u32)],
    row: &'static ProviderRow,
    instance: *mut u8,
    handler: *const u8,
    device: fn() -> u32,
    hooks: &[(u32, *const u8, u32)],
    fills: &[(u32, *const u8, u32)],
    exports: &[(u32, *const u8)],
) -> i32 {
    for &(kind, len) in kinds {
        let r = dk_service_declare_kind(ctx, kind, len, handler);
        if r != 0 {
            return r;
        }
    }
    let r = dk_provider_declare_row(ctx, row, instance);
    if r != 0 {
        return r;
    }
    declare_entries(ctx, device, hooks, fills, exports)
}

#[inline(always)]
unsafe fn declare_entries(
    ctx: *mut u8,
    device: fn() -> u32,
    hooks: &[(u32, *const u8, u32)],
    fills: &[(u32, *const u8, u32)],
    exports: &[(u32, *const u8)],
) -> i32 {
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

/// One patch of the service (`service!` `patches`): declared unless its entry is for another device. The macro calls it once for each entry, so a service without patches carries no
/// code for them.
///
/// # Safety
/// `ctx` is the module context `init` was called with.
#[doc(hidden)]
#[inline(never)]
pub unsafe fn declare_patch(
    ctx: *mut u8,
    device: fn() -> u32,
    site: u32,
    patch: Patch,
    only: u32,
) -> i32 {
    if only != 0 && only != device() {
        return 0;
    }
    dk_service_declare_patch(ctx, site, patch.value, patch.len)
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

/// Whether `parts` (the names `service!` was given in `parts = [...]`) lists `part`: the const check that a service with record kinds adopts the `Records` part.
#[doc(hidden)]
pub const fn parts_adopt(parts: &[&str], part: &str) -> bool {
    let mut i = 0;
    while i < parts.len() {
        let (a, b) = (parts[i].as_bytes(), part.as_bytes());
        if a.len() == b.len() {
            let mut k = 0;
            while k < a.len() && a[k] == b[k] {
                k += 1;
            }
            if k == a.len() {
                return true;
            }
        }
        i += 1;
    }
    false
}
