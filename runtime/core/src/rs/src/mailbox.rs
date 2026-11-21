//! The mailbox commands that carry a selection.

use crate::boot;
use crate::platform::cstr_len;
use dkrt_sdk::*;

/// `CMD_SELECT` / `CMD_SELECT_ONE`: the mailbox argument is a list of NUL-terminated paths ended by an empty one. `copy` 0 or 1
/// writes that copy only; 2 writes both.
pub(crate) unsafe fn mailbox_select(arg: *const u8, copy: u32) -> i32 {
    let mut sel: DkrtSelection = core::mem::zeroed();
    let mut p = 0usize;
    let mut n = 0usize;
    loop {
        let len = cstr_len(arg.add(p), 95 - p.min(95));
        if len == 0 {
            break;
        }
        let e = sel::check_slot(n);
        if e != OK {
            return e;
        }
        if len >= SEL_PATH {
            return E_SEL_PATH;
        }
        let mut i = 0;
        while i < len {
            sel.paths[n][i] = *arg.add(p + i);
            i += 1;
        }
        n += 1;
        p += len + 1;
        if p >= 95 {
            break;
        }
    }
    sel.count = n as u32;
    if copy == 2 {
        boot::selection_set(&sel)
    } else {
        boot::selection_set_one(copy, &sel)
    }
}
