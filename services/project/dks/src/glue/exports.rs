//! The exports of the file: `dk_project_current` and `dk_project_status`.
use super::Owner;
use crate::DK_SVC_OWNER;
use dkrt_svc_rt::{ImportCore, Shared};

/// The state bits (`ST_*`) as of the end of the last tick or scope change: a copy that the owner publishes (`publish`) and `dk_project_status` reads in a masked section. The export
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
