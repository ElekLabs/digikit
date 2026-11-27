//! `dk_project_settings`.
use super::peek::PeekAnswer;
use super::setting::{read_page, Page, ProjectSetting};
use crate::engine::{Env, ProjectService};
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_CHANGED, E_NOT_FOUND};

impl ProjectService {
    /// The saved settings of `module` in project `slot` from entry `cursor` on, into `out`: their number (`>= 0`), and in `next` the cursor of the following page, 0 when the module
    /// has no more. `version` is the write counter the caller saw in the project's summary: another counter answers `E_CHANGED` (the project changed since; nothing is written). The
    /// current project answers at once from its live image. Another slot is read in the background like `get`: `E_AGAIN` until its file was read (one page is kept, at most `PAGE_MAX`
    /// entries, so a page of another project can be shorter than `out`; a request for another module or cursor reads the file again), then the entries, or `E_NOT_FOUND` for a slot
    /// with no usable file and a module the project's module list does not hold.
    pub fn settings<E: Env + ?Sized>(
        &mut self,
        env: &E,
        slot: u32,
        module: &[u8],
        version: u32,
        cursor: u32,
        out: &mut [ProjectSetting],
        next: &mut u32,
    ) -> i32 {
        *next = 0;
        if slot >= 128 {
            return E_NOT_FOUND;
        }
        if self.bound.slot().index() == Some(slot) {
            let counter = self.bound.file().map(|f| f.head.counter).unwrap_or(0);
            if counter != version {
                return E_CHANGED;
            }
            return match read_page(self.bound.image_bytes(), module, cursor, out) {
                Page::NoModule => E_NOT_FOUND,
                Page::Entries { count, more } => {
                    if more {
                        *next = cursor + count as u32;
                    }
                    count as i32
                }
            };
        }
        let answer = self.peek.answer(slot, env.now_ms());
        if answer == PeekAnswer::Gone {
            return E_NOT_FOUND;
        }
        if answer == PeekAnswer::Ready && self.peek.holds_page(module, cursor) {
            if self.peek.summary().counter != version {
                return E_CHANGED;
            }
            let Some((page, more)) = self.peek.page() else {
                return E_NOT_FOUND;
            };
            let count = page.len().min(out.len());
            let mut i = 0;
            while i < count {
                out[i] = page[i];
                i += 1;
            }
            if more || count < page.len() {
                *next = cursor + count as u32;
            }
            return count as i32;
        }
        // a read of another request (or a failed one waiting for its retry time) finishes first: the next call asks for this page
        if answer == PeekAnswer::Wanted {
            return E_AGAIN;
        }
        if env.slot_record(slot).is_none() {
            return E_NOT_FOUND;
        }
        self.peek.want_settings(slot, module, cursor);
        self.queue.add_peek(slot);
        E_AGAIN
    }
}
