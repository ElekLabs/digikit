//! The cached summary of one other project (`dk_project_get`): the slot that was asked for last, and what its file held.
use super::super::restore::ID_MAX;
use super::super::scan::ScanResult;
use super::super::{bytes_equal, is_busy, plan, Env};
use super::setting::{read_page, Page, ProjectSetting};
use super::summary::ProjectSummary;
use crate::dkp::{self, Image, AREA};
use dkrt_sdk::E_NOT_FOUND;

/// After a failed read of the wanted slot (a refused or busy store), the next `get` for it waits this long before it asks again. The retry has its own clock: it never holds back the
/// work of a project.
pub const PEEK_RETRY_MS: u32 = 5000;

/// Where the cached request is.
#[repr(u8)] // zero first: the service is a bss static
#[derive(Clone, Copy, PartialEq, Eq)]
enum PeekState {
    /// Nothing was asked for, or the answer was dropped.
    Idle,
    /// `slot` was asked for: the read is queued or runs.
    Wanted,
    /// `summary` holds the file of `slot`.
    Ready,
    /// `slot` has no usable file.
    Gone,
    /// The read of `slot` failed: it is not asked again before `retry_at`.
    Backoff,
}

/// What the cache says about a slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PeekAnswer {
    /// Not asked for (the cache holds another slot or nothing): the caller asks.
    Other,
    Wanted,
    Ready,
    Gone,
}

/// The most settings entries one read of another project's file keeps for `dk_project_settings` (the caller's `max` may be less).
pub const PAGE_MAX: usize = 4;

/// The most bytes one read of another project's file keeps for `dk_project_read`: a window of the file image (a file up to this size is kept whole, a larger one in windows that
/// start at the offset asked for). It is the service's one extra RAM for reading other projects' files.
pub const RAW_MAX: usize = 4096;

/// One summary: the last slot asked for. A request for another slot replaces it.
pub struct PeekCache {
    slot: u32,
    state: PeekState,
    retry_at: u32,
    summary: ProjectSummary,
    /// A settings page was asked for with the read: the module and the cursor, and what the read found.
    asked: bool,
    module_len: u8,
    module: [u8; ID_MAX],
    cursor: u32,
    found: bool,
    more: bool,
    count: u8,
    page: [ProjectSetting; PAGE_MAX],
    /// Raw bytes were asked for with the read: the window starts at `raw_offset` and holds `raw_len` bytes of a file of `raw_total`.
    raw_asked: bool,
    raw_offset: u32,
    raw_len: u16,
    raw_total: u32,
    raw: [u8; RAW_MAX],
}

impl PeekCache {
    pub const fn new() -> PeekCache {
        PeekCache {
            slot: 0,
            state: PeekState::Idle,
            retry_at: 0,
            summary: ProjectSummary::new(),
            asked: false,
            module_len: 0,
            module: [0; ID_MAX],
            cursor: 0,
            found: false,
            more: false,
            count: 0,
            page: [ProjectSetting::EMPTY; PAGE_MAX],
            raw_asked: false,
            raw_offset: 0,
            raw_len: 0,
            raw_total: 0,
            raw: [0; RAW_MAX],
        }
    }

    /// What the cache holds for `slot` at `now`. A failed read answers `Wanted` until its retry time, then `Other`. (If-let chain, not a `match`: see `Task::prepare`.)
    pub fn answer(&self, slot: u32, now: u32) -> PeekAnswer {
        if self.state == PeekState::Idle || self.slot != slot {
            PeekAnswer::Other
        } else if self.state == PeekState::Backoff {
            if (now.wrapping_sub(self.retry_at) as i32) >= 0 {
                PeekAnswer::Other
            } else {
                PeekAnswer::Wanted
            }
        } else if self.state == PeekState::Ready {
            PeekAnswer::Ready
        } else if self.state == PeekState::Gone {
            PeekAnswer::Gone
        } else {
            PeekAnswer::Wanted
        }
    }

    pub fn summary(&self) -> &ProjectSummary {
        &self.summary
    }

    /// `slot` is asked for: it replaces what the cache held. A read of the same slot again keeps what the earlier requests asked it to keep (the settings page and the raw
    /// window), so one read of the file serves a summary, a page and a raw range together.
    pub fn want(&mut self, slot: u32) {
        if self.slot != slot || self.state == PeekState::Idle {
            self.asked = false;
            self.raw_asked = false;
        }
        self.slot = slot;
        self.state = PeekState::Wanted;
    }

    /// `slot` is asked for with the raw bytes of its file from `offset`: the read keeps a window of up to `RAW_MAX` of them.
    pub fn want_raw(&mut self, slot: u32, offset: u32) {
        self.want(slot);
        self.raw_asked = true;
        self.raw_offset = offset;
    }

    /// Does the kept window cover this offset: it holds the byte there, or it ends the file and the offset is at or past the end?
    pub fn holds_raw(&self, offset: u32) -> bool {
        let end = self.raw_offset as usize + self.raw_len as usize;
        self.raw_asked
            && offset >= self.raw_offset
            && ((offset as usize) < end || end >= self.raw_total as usize)
    }

    /// The bytes of the kept window from `offset` (empty at or past the file's end) and the length of the whole file image. Call after `holds_raw(offset)`.
    pub fn raw(&self, offset: u32) -> (&[u8], u32) {
        let kept = &self.raw[..self.raw_len as usize];
        let from = (offset.saturating_sub(self.raw_offset) as usize).min(kept.len());
        (&kept[from..], self.raw_total)
    }

    /// `slot` is asked for with a page of the settings of `module` from `cursor`: the read of the file keeps that page too. `module` is at most `ID_MAX` bytes.
    pub fn want_settings(&mut self, slot: u32, module: &[u8], cursor: u32) {
        self.want(slot);
        self.asked = true;
        self.module_len = module.len() as u8;
        let mut i = 0;
        while i < module.len() {
            self.module[i] = module[i];
            i += 1;
        }
        self.cursor = cursor;
    }

    /// Does the kept page belong to this request?
    pub fn holds_page(&self, module: &[u8], cursor: u32) -> bool {
        self.asked
            && self.cursor == cursor
            && bytes_equal(&self.module[..self.module_len as usize], module)
    }

    /// The page the read kept: `None` when the file has no such module, else the entries and whether the module has more after them.
    pub fn page(&self) -> Option<(&[ProjectSetting], bool)> {
        if self.found {
            Some((&self.page[..self.count as usize], self.more))
        } else {
            None
        }
    }

    /// The files of `slot` changed: an answer for it is stale.
    pub fn forget(&mut self, slot: u32) {
        if self.slot == slot && (self.state == PeekState::Ready || self.state == PeekState::Gone) {
            self.state = PeekState::Idle;
        }
    }

    /// The read of the wanted slot failed at `now` (a refused or busy store): the next `get` asks again after `PEEK_RETRY_MS`.
    pub fn give_up(&mut self, now: u32) {
        if self.state == PeekState::Wanted {
            self.state = PeekState::Backoff;
            self.retry_at = now.wrapping_add(PEEK_RETRY_MS);
        }
    }

    /// The scan for the wanted slot ended: pick the file the loader would use, read it and keep its summary. A scan for another slot than the wanted one is stale (a newer request
    /// replaced it) and changes nothing. `Err`: the store refused the read of the file with this status (the caller treats it like a refused listing).
    pub fn finish<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        result: &ScanResult,
    ) -> Result<(), i32> {
        if self.state != PeekState::Wanted || self.slot != result.slot() {
            return Ok(());
        }
        let record = result.record();
        let Some(win) = plan::pick_winner(result.files(), &record) else {
            self.state = PeekState::Gone;
            return Ok(());
        };
        let size = env.read(result.files()[win].path.as_bytes(), &mut buffer.0);
        if size == E_NOT_FOUND {
            self.state = PeekState::Gone;
            return Ok(());
        }
        if size < 0 || is_busy(size) {
            return Err(size);
        }
        let Ok(head) = dkp::check(&buffer.0[..(size as usize).min(AREA)], env.device()) else {
            self.state = PeekState::Gone;
            return Ok(());
        };
        self.summary.read_image(&buffer.0, self.slot, head.counter);
        self.found = false;
        self.count = 0;
        if self.asked {
            let module = &self.module[..self.module_len as usize];
            if let Page::Entries { count, more } =
                read_page(&buffer.0, module, self.cursor, &mut self.page)
            {
                self.found = true;
                self.count = count as u8;
                self.more = more;
            }
        }
        if self.raw_asked {
            let total = head.used as usize;
            let from = (self.raw_offset as usize).min(total);
            let n = (total - from).min(RAW_MAX);
            self.raw[..n].copy_from_slice(&buffer.0[from..from + n]);
            self.raw_len = n as u16;
            self.raw_total = total as u32;
        }
        self.state = PeekState::Ready;
        Ok(())
    }
}

impl Default for PeekCache {
    fn default() -> Self {
        Self::new()
    }
}
