//! `dk_project_read`.
use super::peek::{PeekAnswer, RAW_MAX};
use crate::engine::{Env, ProjectService};
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_CHANGED, E_NOT_FOUND};

impl ProjectService {
    /// The raw `.dkp` bytes of project `slot` from `offset` into `out`: their number (`>= 0`, 0 at or past the end), and in `total` the length of the whole file image. `version` is the
    /// write counter the caller saw in the project's summary: another counter answers `E_CHANGED` (nothing is written). Every project, the open one too, is read from its file on the card
    /// in the background like `get`: `E_AGAIN` until the file was read, then the bytes, or `E_NOT_FOUND` for a slot with no usable file. The read keeps a window of up to `RAW_MAX` bytes
    /// of the file image (the file of the open project holds its last write, not unsaved changes), so a call can give less than `out`, and a call inside the window, or for the settings
    /// or the summary of the same slot, reads nothing again; an offset outside it reads the file again.
    pub fn read<E: Env + ?Sized>(
        &mut self,
        env: &E,
        slot: u32,
        version: u32,
        offset: u32,
        out: &mut [u8],
        total: &mut u32,
    ) -> i32 {
        *total = 0;
        if slot >= 128 {
            return E_NOT_FOUND;
        }
        let answer = self.peek.answer(slot, env.now_ms());
        if answer == PeekAnswer::Gone {
            return E_NOT_FOUND;
        }
        if answer == PeekAnswer::Ready && self.peek.holds_raw(offset) {
            if self.peek.summary().counter != version {
                return E_CHANGED;
            }
            let (kept, size) = self.peek.raw(offset);
            *total = size;
            let n = kept.len().min(out.len());
            out[..n].copy_from_slice(&kept[..n]);
            return n as i32;
        }
        // a read of another request (or a failed one waiting for its retry time) finishes first: the next call asks for this range
        if answer == PeekAnswer::Wanted {
            return E_AGAIN;
        }
        if env.slot_record(slot).is_none() {
            return E_NOT_FOUND;
        }
        self.peek.want_raw(slot, offset);
        self.queue.add_peek(slot);
        E_AGAIN
    }
}

const _: () = assert!(RAW_MAX <= u16::MAX as usize);
