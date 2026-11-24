//! The builder call of the shortcuts service as an export of the service file (`wrapper = "builder"` in sdk/sdk.toml): a module's `init` calls it, the loader links it to this
//! entry, and the state it fills is the owner's (`Builder::add`, reached through the file's cell). The call that `patchkit gen` would write as a thin wrapper cannot
//! reach the owner, so this one is written by hand.
use dkrt_sdk::api;
use dkrt_svc_rt::ImportCore;

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_shortcut_add(ctx: *mut u8, def: *const u8) -> i32 {
    // no owner (before `init` made it or after `fini`): the call is outside the service's life, as one outside `init`
    crate::DK_SVC_OWNER
        .with(|shortcuts| shortcuts.builder().add(&ImportCore, ctx, def))
        .unwrap_or(dkrt_sdk::E_CONTEXT)
}

// the entry has the type sdk/sdk.toml gives the import
const _: api::DkShortcutAddFn = dk_shortcut_add;
