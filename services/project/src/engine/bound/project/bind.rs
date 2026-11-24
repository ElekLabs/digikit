//! Binding and unbinding: what a plan asks, the load of a file, an empty project, leaving and a failed load.
use crate::dkp::{self, Ignore, Image, AREA};
use crate::engine::bound::*;
use crate::engine::{Env, SlotRecord};
use dkrt_sdk::logk;
use dkrt_sdk::E_NOT_FOUND;

impl BoundProject {
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
    pub(super) fn load_file<E: Env + ?Sized>(
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
    pub(super) fn bind_empty<E: Env + ?Sized>(
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
    pub(super) fn bind<E: Env + ?Sized>(
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
    pub(super) fn unbind<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        binding: Binding,
    ) {
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
}
