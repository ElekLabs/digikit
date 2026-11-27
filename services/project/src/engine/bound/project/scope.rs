//! The project scope of the bound project: `get`, `put` and `remove`.
use crate::dkp::{self};
use crate::engine::bound::*;
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};
use dkrt_sdk::OK;

impl BoundProject {
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

    pub(super) fn note_change(&mut self) {
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
    }
}
