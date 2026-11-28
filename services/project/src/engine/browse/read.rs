//! `dk_project_read`.
use super::peek::{PeekAnswer, RAW_MAX};
use crate::engine::{Env, ProjectService};
use dkrt_sdk::proto::E_AGAIN;
use dkrt_sdk::{E_CHANGED, E_NOT_FOUND};

/// The bytes of `image` (`used` of them) from `offset`, into `out`.
fn copy_range(image: &[u8], offset: u32, out: &mut [u8]) -> usize {
    let from = (offset as usize).min(image.len());
    let n = (image.len() - from).min(out.len());
    out[..n].copy_from_slice(&image[from..from + n]);
    n
}

impl ProjectService {
    /// The raw `.dkp` bytes of project `slot` from `offset` into `out`: their number (`>= 0`, 0 at or past the end), and in `total` the length of the whole file image. `version` is the
    /// write counter the caller saw in the project's summary: another counter answers `E_CHANGED` (nothing is written). The current project answers at once from its live image as the
    /// service holds it (its header counter and crc are those of the last write). Another slot is read in the background like `get`: `E_AGAIN` until its file was read (up to `RAW_MAX`
    /// bytes are kept, so a call can give less than `out`; a request for another offset reads the file again), then the bytes, or `E_NOT_FOUND` for a slot with no usable file.
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
        if self.bound.slot().index() == Some(slot) {
            let counter = self.bound.file().map(|f| f.head.counter).unwrap_or(0);
            if counter != version {
                return E_CHANGED;
            }
            let used = crate::dkp::head_of(self.bound.image_bytes()).used as usize;
            let image = &self.bound.image_bytes()[..used.min(self.bound.image_bytes().len())];
            *total = image.len() as u32;
            return copy_range(image, offset, out) as i32;
        }
        let answer = self.peek.answer(slot, env.now_ms());
        if answer == PeekAnswer::Gone {
            return E_NOT_FOUND;
        }
        if answer == PeekAnswer::Ready && self.peek.holds_raw(offset) {
            if self.peek.summary().counter != version {
                return E_CHANGED;
            }
            let (kept, size) = self.peek.raw();
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
