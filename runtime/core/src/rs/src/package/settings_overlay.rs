//! A layer over the settings of the modules, given by one service. The runtime has no idea what the layer is for: while a service has set its table
//! (`dkrt_abi::service::SettingsOverlay`: `get`, `put`, `remove` of the module's owner-prefixed entries, with `dkc_settings_overlay_set`), the settings of a module
//! (`dk_settings_get`, `dk_settings_put`, `dk_settings_remove` on a handle from `dk_settings_open`) go through it, over the settings record:
//!
//! - a read returns the layer's value when it holds one, else the record's value;
//! - a write goes to the layer first and then to the record (the record keeps the last used value); a refusal of the layer is the module's answer and the record stays. The layer
//!   answers `E_SET_OFF` when it has nothing to layer now: that is no refusal; every other negative answer is one (`settings::module_put` first asks the
//!   record whether it can take the change, so the record's own refusals come before the layer is written);
//! - the runtime's own keys (effect routes, the selection) are not module settings and stay in the record: they do not pass through here.
//!
//! One provider at a time; its table goes away with the provider's record. The overlay's one user today is described in docs/design/digikit-project.md. All calls run in the
//! UI task. `Provider` owns the table and the record it came from; the free functions at the end are the one static instance the runtime calls.
use core::ptr::addr_of_mut;
use dkrt_abi::service::{SettingsOverlay, SETTINGS_OVERLAY_VERSION};
use dkrt_sdk::proto::{E_SET_OFF, E_SET_VALUE};
use dkrt_sdk::OK;

/// The provider of the layer: the file's record (index + 1; 0 while none) and its table.
pub struct Provider {
    owner: u8,
    table: *const SettingsOverlay,
}

impl Provider {
    pub const fn new() -> Provider {
        Provider { owner: 0, table: core::ptr::null() }
    }

    /// The service file `source` (record index + 1) gives its table. A table of another version, a second one while another file's is set, or one from outside a file is
    /// refused (left unset); null clears the table of that file. True when a table went away.
    pub unsafe fn set(&mut self, source: u8, table: *const SettingsOverlay) -> bool {
        if table.is_null() {
            return self.withdraw_source(source);
        }
        if (*table).version != SETTINGS_OVERLAY_VERSION || self.owner != 0 || source == 0 {
            return false;
        }
        self.owner = source;
        self.table = table;
        false
    }

    /// The file of record `rec` goes away: its table with it. True when it was the provider's.
    pub fn withdraw(&mut self, rec: usize) -> bool {
        self.withdraw_source(rec as u8 + 1)
    }

    fn withdraw_source(&mut self, source: u8) -> bool {
        if source == 0 || self.owner != source {
            return false;
        }
        *self = Provider::new();
        true
    }

    /// Is a table set?
    pub fn loaded(&self) -> bool {
        !self.table.is_null()
    }

    /// The read: `Some(result)` when the layer holds the key (its length, or `E_SET_VALUE` for a small `out`), `None` when the record decides.
    pub unsafe fn get(&self, id: &[u8], key: &[u8], out: &mut [u8]) -> Option<i32> {
        if self.table.is_null() {
            return None;
        }
        let r = ((*self.table).get)(id.as_ptr(), id.len() as u32, key.as_ptr(), key.len() as u32, out.as_mut_ptr(), out.len() as u32);
        if r >= 0 || r == E_SET_VALUE {
            Some(r)
        } else {
            None
        }
    }

    /// The write, before the record takes the value. No table is nothing to do. `E_SET_OFF` is the provider's answer "I have nothing to layer now":
    /// it is `OK` here and the record alone holds the value. Any other negative answer (`E_SET_FULL`, `E_SET_KEY`, `E_SET_VALUE`, an unexpected
    /// code) is the module's answer, with the record unchanged.
    pub unsafe fn put(&self, id: &[u8], key: &[u8], value: &[u8]) -> i32 {
        if self.table.is_null() {
            return OK;
        }
        let r = ((*self.table).put)(id.as_ptr(), id.len() as u32, key.as_ptr(), key.len() as u32, value.as_ptr(), value.len() as u32);
        nothing_to_layer(r)
    }

    /// The remove, before the record drops the key: the answers are those of `put` (`E_SET_OFF` is `OK`, another negative is the module's). A key that is not there is `OK`.
    pub unsafe fn remove(&self, id: &[u8], key: &[u8]) -> i32 {
        if self.table.is_null() {
            return OK;
        }
        let r = ((*self.table).remove)(id.as_ptr(), id.len() as u32, key.as_ptr(), key.len() as u32);
        nothing_to_layer(r)
    }
}

/// The provider's answer as the module's: `E_SET_OFF` (nothing to layer now) and the non-negative answers are `OK`, every other negative passes through.
fn nothing_to_layer(r: i32) -> i32 {
    if r == E_SET_OFF || r >= 0 {
        OK
    } else {
        r
    }
}

static mut PROVIDER: Provider = Provider::new();

fn provider() -> &'static mut Provider {
    unsafe { &mut *addr_of_mut!(PROVIDER) }
}

/// `dkc_settings_overlay_set`: the service file that runs now gives its table (null: takes it away).
pub unsafe fn set(table: *const SettingsOverlay) {
    if provider().set(crate::dlog::current_source(), table) {
        provider_gone();
    }
}

/// The file of record `rec` goes away: drop its table.
pub unsafe fn withdraw(rec: usize) {
    if provider().withdraw(rec) {
        provider_gone();
    }
}

/// No layer changes values any more: no module waits for a reload because of it.
unsafe fn provider_gone() {
    crate::package::settings_reload::forget_all();
}

/// Is a table set?
pub unsafe fn loaded() -> bool {
    provider().loaded()
}

pub unsafe fn get(id: &[u8], key: &[u8], out: &mut [u8]) -> Option<i32> {
    provider().get(id, key, out)
}

pub unsafe fn put(id: &[u8], key: &[u8], value: &[u8]) -> i32 {
    provider().put(id, key, value)
}

pub unsafe fn remove(id: &[u8], key: &[u8]) -> i32 {
    provider().remove(id, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dkrt_sdk::proto::{E_SET_FULL, E_SET_KEY, E_SET_NOKEY};
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::vec::Vec;

    /// The service's table as a map of `<id>/<key>`.
    static STORE: Mutex<BTreeMap<Vec<u8>, Vec<u8>>> = Mutex::new(BTreeMap::new());
    /// What the table's `put` answers instead of storing, when set.
    static PUT_ANSWER: Mutex<Option<i32>> = Mutex::new(None);
    /// What the table's `remove` answers (after removing), when set.
    static REMOVE_ANSWER: Mutex<Option<i32>> = Mutex::new(None);
    fn full(id: &[u8], key: &[u8]) -> Vec<u8> {
        [id, b"/", key].concat()
    }
    unsafe extern "C" fn t_get(id: *const u8, il: u32, key: *const u8, kl: u32, out: *mut u8, cap: u32) -> i32 {
        let k = full(core::slice::from_raw_parts(id, il as usize), core::slice::from_raw_parts(key, kl as usize));
        match STORE.lock().unwrap().get(&k) {
            None => E_SET_NOKEY,
            Some(v) if v.len() > cap as usize => E_SET_VALUE,
            Some(v) => {
                core::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
                v.len() as i32
            }
        }
    }
    unsafe extern "C" fn t_put(id: *const u8, il: u32, key: *const u8, kl: u32, value: *const u8, len: u32) -> i32 {
        let k = full(core::slice::from_raw_parts(id, il as usize), core::slice::from_raw_parts(key, kl as usize));
        if let Some(a) = *PUT_ANSWER.lock().unwrap() {
            return a;
        }
        if len > 100 {
            return E_SET_FULL;
        }
        STORE.lock().unwrap().insert(k, core::slice::from_raw_parts(value, len as usize).to_vec());
        OK
    }
    unsafe extern "C" fn t_remove(id: *const u8, il: u32, key: *const u8, kl: u32) -> i32 {
        let k = full(core::slice::from_raw_parts(id, il as usize), core::slice::from_raw_parts(key, kl as usize));
        STORE.lock().unwrap().remove(&k);
        REMOVE_ANSWER.lock().unwrap().unwrap_or(OK)
    }
    static TABLE: SettingsOverlay = SettingsOverlay { version: SETTINGS_OVERLAY_VERSION, get: t_get, put: t_put, remove: t_remove };
    static OTHER_VERSION: SettingsOverlay = SettingsOverlay { version: 99, get: t_get, put: t_put, remove: t_remove };

    /// The tests own their provider; the store and the answer are shared statics, so they take the runtime's test lock.
    fn fresh() -> (std::sync::MutexGuard<'static, ()>, Provider) {
        let g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        STORE.lock().unwrap().clear();
        *PUT_ANSWER.lock().unwrap() = None;
        *REMOVE_ANSWER.lock().unwrap() = None;
        (g, Provider::new())
    }

    #[test]
    fn a_table_of_another_version_or_a_second_one_is_refused_and_the_record_takes_it_away() {
        let (_g, mut p) = fresh();
        unsafe {
            p.set(1, &OTHER_VERSION);
            assert!(!p.loaded());
            p.set(0, &TABLE);
            assert!(!p.loaded(), "from outside a file");
            p.set(1, &TABLE);
            assert!(p.loaded());
            p.set(2, &TABLE);
            assert!(!p.withdraw(1), "the record of another file does not take it");
            assert!(p.loaded());
            assert!(p.withdraw(0), "the provider's record takes it");
            assert!(!p.loaded());
        }
    }

    #[test]
    fn a_file_clears_its_own_table_only() {
        let (_g, mut p) = fresh();
        unsafe {
            p.set(1, &TABLE);
            assert!(!p.set(2, core::ptr::null()));
            assert!(p.loaded());
            assert!(p.set(1, core::ptr::null()));
            assert!(!p.loaded());
        }
    }

    #[test]
    fn without_a_provider_the_overlay_is_not_there() {
        let (_g, p) = fresh();
        unsafe {
            let mut out = [0u8; 4];
            assert_eq!(p.get(b"test/m", b"k", &mut out), None);
            assert_eq!(p.put(b"test/m", b"k", b"v"), OK);
            assert_eq!(p.remove(b"test/m", b"k"), OK);
            assert!(STORE.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn a_read_takes_the_layer_value_first_and_the_record_when_the_layer_has_none() {
        let (_g, mut p) = fresh();
        unsafe {
            p.set(1, &TABLE);
            let mut out = [0u8; 4];
            // the layer holds nothing: the record decides
            assert_eq!(p.get(b"test/m", b"k", &mut out), None);
            STORE.lock().unwrap().insert(full(b"test/m", b"k"), b"abc".to_vec());
            assert_eq!(p.get(b"test/m", b"k", &mut out), Some(3));
            assert_eq!(&out[..3], b"abc");
            // a value that does not fit is the layer's answer too, not a fall back to a stale record value
            assert_eq!(p.get(b"test/m", b"k", &mut out[..2]), Some(E_SET_VALUE));
            // another module's key is another owner's
            assert_eq!(p.get(b"test/other", b"k", &mut out), None);
        }
    }

    #[test]
    fn a_write_goes_to_the_layer_and_a_remove_takes_the_key_from_it() {
        let (_g, mut p) = fresh();
        unsafe {
            p.set(1, &TABLE);
            assert_eq!(p.put(b"test/m", b"k", b"v"), OK);
            assert_eq!(STORE.lock().unwrap().get(&full(b"test/m", b"k")).map(|v| v.as_slice()), Some(&b"v"[..]));
            assert_eq!(p.remove(b"test/m", b"k"), OK);
            assert!(STORE.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn only_the_nothing_to_layer_answer_is_ok_and_every_other_refusal_is_the_modules() {
        use dkrt_sdk::proto::E_STATE;
        let (_g, mut p) = fresh();
        unsafe {
            p.set(1, &TABLE);
            assert_eq!(p.put(b"test/m", b"k", &[0u8; 101]), E_SET_FULL);
            for e in [E_SET_FULL, E_SET_KEY, E_SET_VALUE, E_STATE] {
                *PUT_ANSWER.lock().unwrap() = Some(e);
                assert_eq!(p.put(b"test/m", b"k", b"v"), e);
                *REMOVE_ANSWER.lock().unwrap() = Some(e);
                assert_eq!(p.remove(b"test/m", b"k"), e);
            }
            // the provider has nothing to layer now: the record alone holds the change
            *PUT_ANSWER.lock().unwrap() = Some(E_SET_OFF);
            assert_eq!(p.put(b"test/m", b"k", b"v"), OK);
            *REMOVE_ANSWER.lock().unwrap() = Some(E_SET_OFF);
            assert_eq!(p.remove(b"test/m", b"k"), OK);
        }
    }
}
