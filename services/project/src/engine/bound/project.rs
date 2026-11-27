//! `BoundProject`: the state and its accessors.
use crate::dkp::{self, Image};
use crate::engine::bound::*;
use dkrt_sdk::E_NOT_FOUND;

mod bind;
mod freeze;
mod install;
mod scope;

/// How long a failed load waits before the service tries it again, quietly.
pub const LOAD_RETRY_MS: u32 = 30_000;

/// What the service holds of the current project. Owns the live image (the file's own bytes, the data the project scope reads and writes), the slot, the live mirror's stamp
/// at the bind, the file the image belongs to, the change counter and the dirty and ignored flags. The tick binds and unbinds it (UI task); the project scope changes the image
/// (UI task too).
pub struct BoundProject {
    /// The live image: the file's own bytes, the current project's data.
    live: Image,
    binding: Binding,
    /// The live mirror's stamp when the image was bound: whether the live project is still the one the image belongs to.
    live_stamp: Option<u32>,
    /// The file the image came from or was last written to.
    file: Option<SourceFile>,
    dirty: bool,
    /// Counts effective puts; a write that froze generation `g` leaves the image clean only if no put came after.
    generation: u32,
    /// A file of the slot was refused (not this project's, or unreadable): the image is empty and the status says so.
    ignored: bool,
    /// The generation of the image that `freeze_into` froze last.
    frozen_generation: u32,
    start: StartBind,
    /// Counts the installs of an image (a project became current, or none): the restore of the module list takes the new list when it sees another count.
    epoch: u32,
    /// The scratch of a value comparison in `install` (a value is at most 1024 bytes).
    scratch: [u8; 1024],
}

impl BoundProject {
    pub const fn new() -> BoundProject {
        BoundProject {
            live: Image::new(),
            binding: Binding::None,
            live_stamp: None,
            file: None,
            dirty: false,
            generation: 0,
            ignored: false,
            frozen_generation: 0,
            start: StartBind::Unnamed,
            epoch: 0,
            scratch: [0; 1024],
        }
    }

    /// The slot of the current project (`Slot::NONE` unless bound).
    pub fn slot(&self) -> Slot {
        self.binding.bound_slot()
    }
    pub fn is_bound(&self) -> bool {
        !self.slot().is_none()
    }
    /// The slot whose load the store refused and that was not loaded since.
    pub fn failed_load(&self) -> Option<u32> {
        self.binding.failed_slot()
    }
    /// The slot whose failed load is due for a quiet retry (once per `LOAD_RETRY_MS`): the next retry counts from now, and a failure of this one counts it again.
    pub fn load_retry(&mut self, now: u32) -> Option<u32> {
        if let Binding::LoadFailed { slot, retry_at } = self.binding {
            if (now.wrapping_sub(retry_at) as i32) >= 0 {
                self.binding = Binding::LoadFailed {
                    slot,
                    retry_at: now.wrapping_add(LOAD_RETRY_MS),
                };
                return Some(slot);
            }
        }
        None
    }
    pub fn live_image(&self) -> &Image {
        &self.live
    }
    pub fn file(&self) -> Option<SourceFile> {
        self.file
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn is_ignored(&self) -> bool {
        self.ignored
    }
    /// The number of images installed so far.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }
    /// The live image's bytes: its module list and its data.
    pub fn image_bytes(&self) -> &[u8] {
        &self.live.0
    }
    /// The facts a planner reads.
    pub fn view(&self) -> BoundView {
        BoundView {
            slot: self.slot(),
            file: self.file,
            dirty: self.dirty,
            has_data: dkp::has_data(&self.live.0),
            used: dkp::head_of(&self.live.0).used,
        }
    }

    /// The name of the current project (16 bytes as read), `E_NOT_FOUND` without one.
    pub fn current(&self) -> Result<(u32, [u8; 16]), i32> {
        match self.slot().index() {
            Some(slot) => Ok((slot, dkp::head_of(&self.live.0).name)),
            None => Err(E_NOT_FOUND),
        }
    }
}
