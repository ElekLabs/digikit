//! `KeyState`: which logical keys (`dk_key`, `sdk/include/dk/gen/keys.h`) are held now. A key number is below `KEY_SLOTS`; `DK_KEY_NONE` (0) is never held.
use dkrt_abi::platform::KEY_SLOTS;

/// The held keys, one bit for each logical key number.
#[derive(Clone, Copy)]
pub struct KeyState {
    held: u64,
}

const _: () = assert!(KEY_SLOTS <= 64, "KeyState holds the keys in one word");

impl KeyState {
    pub const fn new() -> KeyState {
        KeyState { held: 0 }
    }

    fn bit(key: u32) -> u64 {
        if key == 0 || key as usize >= KEY_SLOTS {
            0
        } else {
            1u64 << key
        }
    }

    /// The key went down (also when it is held already).
    pub fn press(&mut self, key: u32) {
        self.held |= KeyState::bit(key);
    }

    /// The key went up.
    pub fn release(&mut self, key: u32) {
        self.held &= !KeyState::bit(key);
    }

    pub fn is_held(&self, key: u32) -> bool {
        self.held & KeyState::bit(key) != 0
    }
}
