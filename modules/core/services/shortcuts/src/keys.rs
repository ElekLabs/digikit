//! `KeyState`: which logical keys (`dk_key`, `sdk/include/dk/gen/keys.h`) are held now, and which of them had their press swallowed (their release and repeats are swallowed
//! too). A key number is below `KEY_SLOTS`; `DK_KEY_NONE` (0) is never held. One function (`bit`) turns a key number into its place in the two sets.
use dkrt_abi::platform::KEY_SLOTS;

/// The held keys and the keys whose press was swallowed, one bit for each logical key number. All-zero bytes are the initial state.
#[derive(Clone, Copy)]
pub struct KeyState {
    held: u64,
    swallowed: u64,
}

const _: () = assert!(KEY_SLOTS <= 64, "KeyState holds the keys in one word");

impl KeyState {
    pub const fn new() -> KeyState {
        KeyState {
            held: 0,
            swallowed: 0,
        }
    }

    /// The bit of `key`; 0 for `DK_KEY_NONE` and a number past the table.
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

    /// The press of `key` was swallowed: its repeats and its release are swallowed too.
    pub fn swallow(&mut self, key: u32) {
        self.swallowed |= KeyState::bit(key);
    }

    pub fn is_swallowed(&self, key: u32) -> bool {
        self.swallowed & KeyState::bit(key) != 0
    }

    /// The release of `key`: true when its press was swallowed (the release is swallowed, once).
    pub fn take_swallowed(&mut self, key: u32) -> bool {
        let was = self.is_swallowed(key);
        self.swallowed &= !KeyState::bit(key);
        was
    }
}
