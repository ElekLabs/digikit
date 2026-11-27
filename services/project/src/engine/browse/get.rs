//! `dk_project_get`.
use super::peek::PeekAnswer;
use super::summary::ProjectSummary;
use crate::engine::{Env, ProjectService};
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_NOT_FOUND, OK};

impl ProjectService {
    /// The summary of project `slot` into `out`. The current project is answered at once from its live image (`OK`). Another slot is read in the background: the first call queues the
    /// read of its file and answers `E_AGAIN`, the calls after it `E_AGAIN` until the read ended, then `OK`, or `E_NOT_FOUND` when the slot has no usable file (also a slot the firmware
    /// does not list). One summary is kept: a request for another slot replaces it. The `state` of a module is `MODULE_LOADED` or `MODULE_UNKNOWN` (the card is not read).
    pub fn get<E: Env + ?Sized>(&mut self, env: &E, slot: u32, out: &mut ProjectSummary) -> i32 {
        if slot >= 128 {
            return E_NOT_FOUND;
        }
        if self.bound.slot().index() == Some(slot) {
            let counter = self.bound.file().map(|f| f.head.counter).unwrap_or(0);
            out.read_image(self.bound.image_bytes(), slot, counter);
            out.mark_loaded(env);
            return OK;
        }
        let answer = self.peek.answer(slot, env.now_ms());
        if answer == PeekAnswer::Ready {
            *out = *self.peek.summary();
            out.mark_loaded(env);
            OK
        } else if answer == PeekAnswer::Gone {
            E_NOT_FOUND
        } else if answer == PeekAnswer::Wanted {
            E_AGAIN
        } else if env.slot_record(slot).is_none() {
            E_NOT_FOUND
        } else {
            self.peek.want(slot);
            self.queue.add_peek(slot);
            E_AGAIN
        }
    }
}
