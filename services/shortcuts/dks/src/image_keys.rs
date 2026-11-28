//! `ImageKeys`: the subscription of the key routine over the image's table of key subscribers (`core/input`, `input_keys_info`), as the owner's `KeyEvents`. The service finds the table at
//! `start` (`KeyTable::find`) and calls `key_subscribe(routine)`; `key_unsubscribe(routine)` takes it out at `stop`. The ColdFire build only: a host build (the tests of the file) has the owner
//! over `UnwiredKeys`.
use crate::key_routine::dk_shortcuts_key;
use dkrt_sdk::{E_EXISTS, E_NO_REGISTRY, E_TABLE_FULL};
use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::Status;
use dkrt_svc_shortcuts::key_events::KeyEvents;
use dkrt_svc_shortcuts::key_table::KeyTable;
use dkrt_svc_shortcuts::shortcuts::Shortcuts;

/// The owner object of the service file: the service over the image's table of key subscribers.
pub type Owner = Shortcuts<ImageKeys>;

/// The bound table, or none: all-zero bytes are the unsubscribed state.
pub struct ImageKeys {
    /// The entry of `key_subscribe` and of `key_unsubscribe`; 0 before the table is found.
    subscribe: u32,
    unsubscribe: u32,
    subscribed: bool,
}

/// `KEYS_FULL` and `KEYS_DUPLICATE` of `key_subscribers.h`.
const KEYS_FULL: i32 = -1;
const KEYS_DUPLICATE: i32 = -2;

// SAFETY: integers and a bool: all-zero bytes are the unsubscribed state.
unsafe impl KeyEvents for ImageKeys {
    const IDLE: ImageKeys = ImageKeys {
        subscribe: 0,
        unsubscribe: 0,
        subscribed: false,
    };

    fn subscribe<C: CoreApi>(&mut self, core: &C) -> Result<(), Status> {
        let table = KeyTable::find(core).ok_or(E_NO_REGISTRY)?;
        // SAFETY: `subscribe` is the entry of `key_subscribe` of the image, called in the UI task with the address of this file's routine
        let slot = unsafe {
            let f: extern "C" fn(usize) -> i32 = core::mem::transmute(table.subscribe() as usize);
            f(dk_shortcuts_key as usize)
        };
        match slot {
            s if s >= 0 => {
                self.subscribe = table.subscribe();
                self.unsubscribe = table.unsubscribe();
                self.subscribed = true;
                Ok(())
            }
            KEYS_FULL => Err(E_TABLE_FULL),
            KEYS_DUPLICATE => Err(E_EXISTS),
            _ => Err(E_NO_REGISTRY),
        }
    }

    fn unsubscribe(&mut self) {
        if self.subscribed {
            // SAFETY: `unsubscribe` is the entry of `key_unsubscribe` of the image, called in the UI task
            unsafe {
                let f: extern "C" fn(usize) -> i32 =
                    core::mem::transmute(self.unsubscribe as usize);
                f(dk_shortcuts_key as usize);
            }
            self.subscribed = false;
        }
    }
}
