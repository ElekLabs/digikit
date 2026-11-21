//! The project that is current: its slot, the live image with its bookkeeping, the file it came from. UI task only (the project scope's `get`, `put` and `remove` run there too).
use super::restore::{ModuleSet, MODULES_MAX};
use super::{bytes_equal, Env, SlotRecord};
use crate::dkp::{self, Head, Ignore, Image, AREA};
use crate::name::{self, Path};
use dkrt_sdk::logk;
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};
use dkrt_sdk::{E_NOT_FOUND, OK};

/// How long a failed load waits before the service tries it again, quietly.
pub const LOAD_RETRY_MS: u32 = 30_000;

/// The slot of the firmware's live mirror: its stamp is the stamp of the project being edited.
pub const LIVE_MIRROR: u32 = 0x80;

/// The slot of the current project, or none. Stored as the slot plus one, so that none is zero and the whole state of the service is zero at the start (a static with a non-zero
/// value would put 35 KB of zeros in the file's data).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Slot(u32);

impl Slot {
    pub const NONE: Slot = Slot(0);
    pub const fn new(index: u32) -> Slot {
        Slot(index + 1)
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
    /// The slot index, `None` for none.
    pub fn index(self) -> Option<u32> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0 - 1)
        }
    }
    /// The slot index, -1 for none.
    pub fn as_i32(self) -> i32 {
        self.0 as i32 - 1
    }
}

/// The file a bound project came from or was last written to: its header fields and its path.
#[derive(Clone, Copy)]
pub struct SourceFile {
    pub head: Head,
    pub path: Path,
}

/// What a plan asks of the bound project; [`BoundProject::apply`] does it.
#[derive(Clone, Copy)]
pub enum Effect {
    /// Make `slot` current from `file`, a file of the slot that passed the checks (`record` is the firmware's record of the slot).
    BindFile {
        slot: u32,
        record: SlotRecord,
        file: SourceFile,
    },
    /// Make `slot` current with an empty image; `ignored` is why a file of the slot was refused (logged and warned once).
    BindEmpty {
        slot: u32,
        record: SlotRecord,
        ignored: Option<Ignore>,
    },
    /// The current project's slot was cleared: its image is empty and its file gone.
    EmptyCurrent { slot: u32, record: SlotRecord },
    /// A stock save into `slot` is about to write: when that is not the current slot the image goes with the save.
    AdoptForSave { slot: u32 },
}

/// What [`BoundProject::apply`] came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applied {
    /// Done: the plan's tasks may run.
    Done,
    /// The file of a load vanished or changed between the listing and the read: the slot was bound empty and the plan ends.
    Vanished,
    /// The store refused the read of a load's file with this status (busy or not): nothing changed, the caller handles it like a refused listing.
    Refused(i32),
}

/// What a planner reads of the bound project: a copy of the facts, so that a plan changes nothing.
#[derive(Clone, Copy)]
pub struct BoundView {
    pub slot: Slot,
    /// The file the image came from or was last written to.
    pub file: Option<SourceFile>,
    pub dirty: bool,
    /// Does the image hold data worth a file?
    pub has_data: bool,
    /// The bytes of the image's file.
    pub used: u32,
}

impl BoundView {
    /// The view after `Effect::AdoptForSave`, the one rule for it: a save into another slot takes the image with it, so the slot has no file yet and the image counts as changed.
    /// The planner reads it and `BoundProject::apply` installs it.
    pub fn adopted_for_save(self, slot: u32) -> BoundView {
        if self.slot.as_i32() == slot as i32 {
            return self;
        }
        BoundView {
            slot: Slot::new(slot),
            file: None,
            dirty: true,
            ..self
        }
    }

    /// Does the file the image came from carry the slot, stamp and name of the firmware's `record`?
    pub fn record_matches_file(&self, slot: u32, record: &SlotRecord) -> bool {
        if let Some(file) = self.file {
            return file.head.slot as u32 == slot
                && file.head.stamp == record.stamp
                && file.head.name == record.name;
        }
        false
    }
}

/// What the service knows of the current project. The scope is on only for `Bound`; `LoadFailed` is off too (module writes go to the settings record), but remembers the slot whose
/// load the store refused: the firmware stays on that slot and calls the setter no more, so a stock save into it must not be taken for a save of an empty project.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)] // zero first: the service is a bss static
enum Binding {
    /// No current project (the start, or the setter left the project).
    None,
    /// The project of this slot is current: the image belongs to it.
    Bound(u32),
    /// The load of `slot` ended with a store error; the image is empty and belongs to no project. The load is tried again quietly at `retry_at` (ms, wraps).
    LoadFailed { slot: u32, retry_at: u32 },
}

impl Binding {
    /// The slot of the bound project (`Slot::NONE` unless `Bound`).
    fn bound_slot(self) -> Slot {
        if let Binding::Bound(slot) = self {
            return Slot::new(slot);
        }
        Slot::NONE
    }
    /// The slot whose load failed.
    fn failed_slot(self) -> Option<u32> {
        if let Binding::LoadFailed { slot, .. } = self {
            return Some(slot);
        }
        None
    }
}

/// Is the project that was current at the start named yet? The stock setter is not called for a project that was already current, so the first save would look like a save-as
/// and archive its file: until a setter call or a read of the firmware names the project, the tick asks the firmware for its slot.
#[derive(Clone, Copy)]
#[repr(u8)] // zero first: the service is a bss static
enum StartBind {
    Unnamed,
    Named,
}

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

/// Does the entry (`owner`, `key`, `value`) of one image read another value than with the image `other` current? A module reads the image's value, else the settings record's,
/// else nothing. `scratch` holds the value that is read.
fn entry_differs<E: Env + ?Sized>(
    env: &mut E,
    scratch: &mut [u8],
    other: &[u8],
    owner: &[u8],
    key: &[u8],
    value: &[u8],
) -> bool {
    let bare = &key[owner.len() + 1..];
    let mut read = dkp::get(other, owner, bare, scratch);
    if read < 0 {
        read = env.record_get(key, scratch);
    }
    !(read >= 0 && &scratch[..read as usize] == value)
}

/// Did an entry of `owner` that `install` looked at before `(pass, entry_at)` read another value? (For an owner that does not fit the table of named owners.) All entries of the old
/// image come first (pass 0, compared against the new image), then those of the new image (pass 1, compared against the old one).
fn differed_before<E: Env + ?Sized>(
    env: &mut E,
    scratch: &mut [u8],
    (old, new): (&[u8], &[u8]),
    (pass, entry_at): (usize, usize),
    owner: &[u8],
) -> bool {
    let mut earlier = 0;
    while earlier <= pass {
        let (from, other) = if earlier == 0 { (old, new) } else { (new, old) };
        let limit = if earlier == pass {
            entry_at
        } else {
            usize::MAX
        };
        let mut index = 0;
        for (entry_owner, key, value) in dkp::entries(from) {
            if index >= limit {
                break;
            }
            index += 1;
            if bytes_equal(entry_owner, owner)
                && entry_differs(env, scratch, other, entry_owner, key, value)
            {
                return true;
            }
        }
        earlier += 1;
    }
    false
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

    // ---- the project scope (UI task) -------------------------------------------------------------------------------------------------

    /// `dk_settings_get` on the project scope: no current project is `E_SET_NOKEY`.
    pub fn get(&self, owner: &[u8], key: &[u8], out: &mut [u8]) -> i32 {
        if !self.is_bound() {
            return E_SET_NOKEY;
        }
        dkp::get(&self.live.0, owner, key, out)
    }

    /// `dk_settings_put`: no current project is `E_SET_OFF`; a put of the stored value changes nothing and sets no dirty flag.
    pub fn put(&mut self, owner: &[u8], key: &[u8], value: &[u8]) -> i32 {
        if !self.is_bound() {
            return E_SET_OFF;
        }
        match dkp::put(&mut self.live.0, owner, key, value) {
            Ok(true) => {
                self.note_change();
                OK
            }
            Ok(false) => OK,
            Err(status) => status,
        }
    }

    pub fn remove(&mut self, owner: &[u8], key: &[u8]) -> i32 {
        if !self.is_bound() {
            return E_SET_OFF;
        }
        match dkp::remove(&mut self.live.0, owner, key) {
            Ok(true) => {
                self.note_change();
                OK
            }
            Ok(false) => OK,
            Err(status) => status,
        }
    }

    fn note_change(&mut self) {
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
    }

    // ---- bind and unbind (the tick) --------------------------------------------------------------------------------------------------

    /// Is the live project still the one the image belongs to? True also when the firmware's live stamp cannot be read.
    pub fn image_matches_live<E: Env + ?Sized>(&self, env: &E) -> bool {
        match (self.live_stamp, env.slot_record(LIVE_MIRROR)) {
            (Some(stamp), Some(mirror)) => stamp == mirror.stamp,
            _ => true,
        }
    }

    fn remember_live_stamp<E: Env + ?Sized>(&mut self, env: &E) {
        self.live_stamp = env.slot_record(LIVE_MIRROR).map(|mirror| mirror.stamp);
    }

    /// The new project's image is in `frozen`: tell the runtime which modules' settings it changes, then it becomes the live image. A module sees the effective value of a key:
    /// the project's, else the settings record's. So a key differs between two images when both hold it with other values, or one holds it with a value other than the record's
    /// and the other does not hold it. (No current project before: the old image counts as empty.)
    fn install<E: Env + ?Sized>(&mut self, env: &mut E, frozen: &Image) {
        // owners already named, up to 16; an owner past the table is found by looking at the earlier entries of the same pass instead
        let mut seen = [[0u8; 32]; 16];
        let mut seen_len = [0usize; 16];
        let mut seen_count = 0;
        let had_project = self.is_bound();
        let BoundProject { live, scratch, .. } = self;
        // an image that holds nothing: a header (no 16 KiB on the stack)
        let mut empty = [0u8; dkp::HEADER];
        dkp::reset(&mut empty, 0, 0, &[0; 16], 0);
        let old: &[u8] = if had_project { &live.0 } else { &empty };
        let new: &[u8] = &frozen.0;
        let mut pass = 0;
        while pass < 2 {
            let (from, other) = if pass == 0 { (old, new) } else { (new, old) };
            let mut entry_at = 0;
            for (owner, key, value) in dkp::entries(from) {
                entry_at += 1;
                if !entry_differs(env, &mut scratch[..], other, owner, key, value) {
                    continue;
                }
                let mut named = false;
                let mut at = 0;
                while at < seen_count {
                    if bytes_equal(&seen[at][..seen_len[at]], owner) {
                        named = true;
                    }
                    at += 1;
                }
                if named {
                    continue;
                }
                if seen_count < 16 && owner.len() <= 32 {
                    seen[seen_count][..owner.len()].copy_from_slice(owner);
                    seen_len[seen_count] = owner.len();
                    seen_count += 1;
                } else if differed_before(
                    env,
                    &mut scratch[..],
                    (old, new),
                    (pass, entry_at - 1),
                    owner,
                ) {
                    // the table is full: an earlier entry of this owner differed, so the runtime was told
                    continue;
                }
                env.changed(owner);
            }
            pass += 1;
        }
        let used = dkp::head_of(&frozen.0).used as usize;
        self.live.0[..used].copy_from_slice(&frozen.0[..used]);
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// A stock save is about to write the image: its module list becomes the content modules that are loaded now (a list that differs counts as a change). An image that is too
    /// full for the list keeps the list it has.
    pub fn record_modules<E: Env + ?Sized>(&mut self, env: &E) {
        let loaded = ModuleSet::of_loaded(env);
        if loaded.same_as(&ModuleSet::of_image(&self.live.0)) {
            return;
        }
        let mut list: [(&[u8], u32); MODULES_MAX] = [(&[], 0); MODULES_MAX];
        let count = loaded.list(&mut list);
        if dkp::set_modules(&mut self.live.0, &list[..count]).is_ok() {
            self.note_change();
        }
    }

    /// The project that was current at the start, until it is named: a setter call names it (`None`), else the firmware's slot once its record is listed (`Some(slot)`, to load).
    pub fn start_slot<E: Env + ?Sized>(&mut self, env: &E, setter: Option<u32>) -> Option<u32> {
        if let StartBind::Named = self.start {
            return None;
        }
        if setter.is_some() {
            self.start = StartBind::Named;
            return None;
        }
        if let Some(slot) = env.firmware_slot() {
            if env.slot_record(slot).is_some() {
                self.start = StartBind::Named;
                return Some(slot);
            }
        }
        None
    }

    /// Do what a plan asks (`Applied`). `buffer` is scratch. (If-let chain, not a `match`: see `Task::prepare`.)
    pub fn apply<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        effect: Effect,
    ) -> Applied {
        if let Effect::BindFile { slot, record, file } = effect {
            let applied = self.load_file(env, buffer, slot, file);
            if applied == Applied::Vanished {
                // it vanished or changed between the two reads: no data this time
                self.bind_empty(env, buffer, slot, &record, None);
            }
            return applied;
        }
        if let Effect::BindEmpty {
            slot,
            record,
            ignored,
        } = effect
        {
            self.bind_empty(env, buffer, slot, &record, ignored);
        } else if let Effect::EmptyCurrent { slot, record } = effect {
            dkp::reset(
                &mut buffer.0,
                env.device(),
                slot as u8,
                &record.name,
                record.stamp,
            );
            self.install(env, buffer);
            self.file = None;
            self.dirty = false;
        } else if let Effect::AdoptForSave { slot } = effect {
            let adopted = self.view().adopted_for_save(slot);
            if let Some(index) = adopted.slot.index() {
                self.binding = Binding::Bound(index);
            }
            self.file = adopted.file;
            self.dirty = adopted.dirty;
            self.remember_live_stamp(env);
        }
        Applied::Done
    }

    /// The winner's bytes become the image and the project is bound to the file; LOADED follows. A read the store refuses (busy or any other status) is `Refused`: only a file
    /// that is gone (`E_NOT_FOUND`) or no longer the one the listing checked (it fails the checks, or its header is another) is `Vanished`.
    fn load_file<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        slot: u32,
        file: SourceFile,
    ) -> Applied {
        let size = env.read(file.path.as_bytes(), &mut buffer.0);
        if size == E_NOT_FOUND {
            return Applied::Vanished;
        }
        if size < 0 {
            return Applied::Refused(size);
        }
        match dkp::check(&buffer.0[..(size as usize).min(AREA)], env.device()) {
            Ok(head) if size as usize <= AREA && head == file.head => {}
            _ => return Applied::Vanished,
        }
        self.install(env, buffer);
        self.bind(env, slot, Some(file), false);
        let mut modules = 0;
        for _ in dkp::modules(&self.live.0) {
            modules += 1;
        }
        env.log(
            logk::PROJECT_LOADED,
            [slot, file.head.used, modules, file.head.counter, 0],
        );
        Applied::Done
    }

    /// The slot becomes current with an empty image; LOADED follows, and when a file of the slot was refused (`ignored`) one IGNORED event and one warning.
    fn bind_empty<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        slot: u32,
        record: &SlotRecord,
        ignored: Option<Ignore>,
    ) {
        dkp::reset(
            &mut buffer.0,
            env.device(),
            slot as u8,
            &record.name,
            record.stamp,
        );
        self.install(env, buffer);
        self.bind(env, slot, None, ignored.is_some());
        env.log(logk::PROJECT_LOADED, [slot, 0, 0, 0, 0]);
        if let Some(reason) = ignored {
            env.log(logk::PROJECT_IGNORED, [slot, reason as u32, 0, 0, 0]);
            env.toast(reason as u32);
        }
    }

    /// The slot becomes current (its image was installed): from `file`, or with an empty image (`None`; `ignored` says a file of the slot was refused).
    fn bind<E: Env + ?Sized>(
        &mut self,
        env: &E,
        slot: u32,
        file: Option<SourceFile>,
        ignored: bool,
    ) {
        self.binding = Binding::Bound(slot);
        self.file = file;
        self.dirty = false;
        self.ignored = ignored;
        self.remember_live_stamp(env);
    }

    /// No current project: an empty image is installed and the scope is off. (Module writes go to the settings record only.)
    fn unbind<E: Env + ?Sized>(&mut self, env: &mut E, buffer: &mut Image, binding: Binding) {
        dkp::reset(&mut buffer.0, env.device(), 0, &[0; 16], 0);
        self.install(env, buffer);
        self.binding = binding;
        self.file = None;
        self.dirty = false;
        self.ignored = false;
        self.remember_live_stamp(env);
    }

    /// The stock setter got a value that is no slot (-1 or the live mirror): no current project; LOADED carries that value.
    pub fn leave<E: Env + ?Sized>(&mut self, env: &mut E, buffer: &mut Image, setter_value: u32) {
        self.unbind(env, buffer, Binding::None);
        env.log(logk::PROJECT_LOADED, [setter_value, 0, 0, 0, 0]);
    }

    /// The store refused the load of `slot`: no current project, as for `leave`, and the slot is remembered as a failed load (`Binding::LoadFailed`) with its next quiet retry
    /// `LOAD_RETRY_MS` after `now`. A second failure of the same slot only moves the retry.
    pub fn fail_load<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        slot: u32,
        now: u32,
    ) {
        let binding = Binding::LoadFailed {
            slot,
            retry_at: now.wrapping_add(LOAD_RETRY_MS),
        };
        if self.binding.failed_slot() == Some(slot) {
            self.binding = binding;
        } else {
            self.unbind(env, buffer, binding);
        }
    }

    /// The file that was refused is gone (archived): the project is no longer ignored.
    pub fn clear_ignored(&mut self) {
        self.ignored = false;
    }

    /// Freeze the image for a write: its header gets the slot, name and stamp as the firmware lists them now and the write counter `counter`, then its bytes are copied to
    /// `buffer`. Returns their number. A put after this leaves the image dirty (`mark_written`).
    pub fn freeze_into<E: Env + ?Sized>(
        &mut self,
        env: &E,
        buffer: &mut Image,
        slot: u8,
        record: &SlotRecord,
        counter: u32,
    ) -> u32 {
        let mut head = dkp::head_of(&self.live.0);
        head.device = env.device();
        head.slot = slot;
        head.name = record.name;
        head.name_len = name::trimmed_len_raw(&record.name) as u8;
        head.stamp = record.stamp;
        dkp::set_head(&mut self.live.0, &head);
        dkp::seal(&mut self.live.0, counter);
        let used = dkp::head_of(&self.live.0).used as usize;
        buffer.0[..used].copy_from_slice(&self.live.0[..used]);
        self.frozen_generation = self.generation;
        used as u32
    }

    /// The bytes that `freeze_into` froze were written as `to`: the image belongs to that file now, and is clean unless a put came after the freeze.
    pub fn mark_written(&mut self, to: Path, slot: u8, record: &SlotRecord, counter: u32) {
        let mut head = dkp::head_of(&self.live.0);
        head.slot = slot;
        head.stamp = record.stamp;
        head.name = record.name;
        head.counter = counter;
        self.file = Some(SourceFile { head, path: to });
        if self.generation == self.frozen_generation {
            self.dirty = false;
        }
    }
}
