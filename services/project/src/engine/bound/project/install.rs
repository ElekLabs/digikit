//! Installing an image: which modules' settings it changes, and the stock save's module list.
use crate::dkp::{self, Image};
use crate::engine::bound::*;
use crate::engine::restore::{ModuleSet, MODULES_MAX};
use crate::engine::{bytes_equal, Env};

/// Does the entry (`owner`, `key`, `value`) of one image read another value than with the image `other` current? A module reads the image's value, else the settings record's,
/// else nothing. `scratch` holds the value that is read.
pub(super) fn entry_differs<E: Env + ?Sized>(
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
pub(super) fn differed_before<E: Env + ?Sized>(
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
    // ---- bind and unbind (the tick) --------------------------------------------------------------------------------------------------

    /// Is the live project still the one the image belongs to? True also when the firmware's live stamp cannot be read.
    pub fn image_matches_live<E: Env + ?Sized>(&self, env: &E) -> bool {
        match (self.live_stamp, env.slot_record(LIVE_MIRROR)) {
            (Some(stamp), Some(mirror)) => stamp == mirror.stamp,
            _ => true,
        }
    }

    pub(super) fn remember_live_stamp<E: Env + ?Sized>(&mut self, env: &E) {
        self.live_stamp = env.slot_record(LIVE_MIRROR).map(|mirror| mirror.stamp);
    }

    /// The new project's image is in `frozen`: tell the runtime which modules' settings it changes, then it becomes the live image. A module sees the effective value of a key:
    /// the project's, else the settings record's. So a key differs between two images when both hold it with other values, or one holds it with a value other than the record's
    /// and the other does not hold it. (No current project before: the old image counts as empty.)
    pub(super) fn install<E: Env + ?Sized>(&mut self, env: &mut E, frozen: &Image) {
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
}
