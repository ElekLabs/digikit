//! The exports of the file: `dk_project_current`, `dk_project_status`, `dk_project_list`, `dk_project_get` and `dk_project_settings`.
use super::Owner;
use crate::DK_SVC_OWNER;
use dkrt_svc_project::engine::{ProjectEntry, ProjectSetting, ProjectSummary};
use dkrt_svc_rt::{ImportCore, Shared};

/// The state bits as of the end of the last tick or scope change: a copy that the owner publishes (`publish`) and `dk_project_status` reads in a masked section. The export
/// answers from any task, also while the owner is inside a call (the UI task in a tick, with the MIDI task calling), where the owner's guard would refuse it. Not owner state: the
/// owner never holds a reference to it across a call.
pub static DK_STATUS: Shared<u32> = Shared::new(0);

/// Publish the owner's state bits. Called by the entries that can change them (the tick and the scope calls), inside their call of the owner.
pub(super) fn publish(p: &Owner) {
    DK_STATUS.with(&ImportCore, |s| *s = p.status());
}

/// Forget the copy (the owner starts: nothing is bound).
pub(super) fn reset_status() {
    DK_STATUS.with(&ImportCore, |s| *s = 0);
}

/// `dk_project_current` (export of the file): the slot and the name of the current project, `E_NOT_FOUND` without one.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_current(slot: *mut u32, name: *mut u8) -> i32 {
    // no owner: no current project
    match DK_SVC_OWNER
        .with(|p| p.current())
        .unwrap_or(Err(dkrt_sdk::E_NOT_FOUND))
    {
        Ok((s, n)) => {
            if !slot.is_null() {
                *slot = s;
            }
            if !name.is_null() {
                let mut i = 0;
                while i < 16 {
                    *name.add(i) = n[i];
                    i += 1;
                }
            }
            0
        }
        Err(e) => e,
    }
}

/// `dk_project_status` (export of the file): the state bits, 0 without a service. A read of the published copy, so a call from the MIDI task gets an answer while the owner is busy.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_status() -> u32 {
    if DK_SVC_OWNER.is_live() {
        DK_STATUS.with(&ImportCore, |s| *s)
    } else {
        0
    }
}

/// The most entries of one `dk_project_list` call: the firmware has 128 slots.
const LIST_MAX: u32 = 128;

/// `dk_project_list` (export of the file): the entries of the firmware's projects from slot `first_slot` upward, at most `max`, into `out` (`dk_project_entry[max]`): their number. No
/// I/O (see `ProjectService::list`). 0 without a service, an `out` that is null or a `max` of 0.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_list(first_slot: u32, out: *mut u8, max: u32) -> i32 {
    if out.is_null() || max == 0 {
        return 0;
    }
    // SAFETY: the caller's array of `max` entries (the struct has no padding and align 1).
    let entries =
        core::slice::from_raw_parts_mut(out as *mut ProjectEntry, max.min(LIST_MAX) as usize);
    DK_SVC_OWNER
        .with(|p| p.list(first_slot, entries) as i32)
        .unwrap_or(0)
}

/// `dk_project_get` (export of the file): the summary of project `slot` into `out` (`dk_project_summary`). 0, `DK_E_AGAIN` while the file of another slot is read, `DK_E_NOT_FOUND`
/// (no file, no such slot, no service), `DK_E_RANGE` for a null `out`.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_get(slot: u32, out: *mut u8) -> i32 {
    if out.is_null() {
        return dkrt_sdk::E_RANGE;
    }
    DK_SVC_OWNER
        .with(|p| {
            // SAFETY: the caller's `dk_project_summary`: ColdFire aligns a u32 to 2 bytes and the layout uses explicit pads, so the struct is 2-byte aligned.
            let summary = &mut *(out as *mut ProjectSummary);
            p.summary(slot, summary)
        })
        .unwrap_or(dkrt_sdk::E_NOT_FOUND)
}

/// The longest module id (`<author>/<name>`).
const ID_MAX: u32 = 31;

/// `dk_project_settings` (export of the file): the saved settings of the module `module_id` (`id_len` bytes) of project `slot` from entry `cursor` on into `out` (`dk_project_setting[max]`):
/// their number, and in `*next` (when not null) the cursor of the following page, 0 for none. `version` is the project's write counter (`dk_project_get`): another counter
/// answers `DK_E_CHANGED`. `DK_E_AGAIN` while the file of another slot is read, `DK_E_NOT_FOUND` (no file, no such module, no service), `DK_E_RANGE` for a null `module_id` or `out`, a
/// `max` of 0 or an id that is empty or over 31 bytes.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_settings(
    slot: u32,
    module_id: *const u8,
    id_len: u32,
    version: u32,
    cursor: u32,
    out: *mut u8,
    max: u32,
    next: *mut u32,
) -> i32 {
    if out.is_null() || module_id.is_null() || max == 0 || id_len == 0 || id_len > ID_MAX {
        return dkrt_sdk::E_RANGE;
    }
    // SAFETY: the caller's id bytes and array of `max` entries (the struct has no padding and align 2: ColdFire aligns a u16 to 2 bytes).
    let module = core::slice::from_raw_parts(module_id, id_len as usize);
    let entries =
        core::slice::from_raw_parts_mut(out as *mut ProjectSetting, max.min(MAX_ENTRIES) as usize);
    let mut after = 0u32;
    let status = DK_SVC_OWNER
        .with(|p| p.settings(slot, module, version, cursor, entries, &mut after))
        .unwrap_or(dkrt_sdk::E_NOT_FOUND);
    if !next.is_null() {
        // the caller's word: ColdFire aligns a u32 to 2 bytes, so write it unaligned
        next.write_unaligned(after);
    }
    status
}

/// The most entries of one `dk_project_settings` call.
const MAX_ENTRIES: u32 = 16;
