/// Mailbox commands (emulator and test use). `CMD_LOAD` calls `dkrt_load`, which answers `E_BOOT_ONLY` once the boot is over; command
/// 2 was `CMD_UNLOAD` (archived with hot reload, tag `digikit-hot-reload-v1`) and is answered `E_FORMAT`.
pub const CMD_LOAD: u32 = 1;
/// `arg`: store paths, each NUL terminated, the list ended by an empty path (an empty list clears the selection):
/// `dkrt_selection_set`. The answer is its result.
pub const CMD_SELECT: u32 = 3;
/// `dkrt_restart`; the answer is only seen when it refuses.
pub const CMD_RESTART: u32 = 4;
/// Write ONE copy of the selection record from `arg` (`arg[0]` = copy 0 or 1, then the paths as for `CMD_SELECT`) with sequence
/// number `seq + 1`: lets a test make the state between the two writes of `dkrt_selection_set` (a power cut).
pub const CMD_SELECT_ONE: u32 = 5;
/// Run `dkrt_health` into the exported `DKRT_HEALTH` (the proof reads it; `result` is 0).
pub const CMD_HEALTH: u32 = 6;
/// Test hook: `arg[0..4]` = sector, `arg[4..8]` = byte count (big endian); the runtime calls its one card-write function with the staging
/// buffer and answers its result (0, `E_RANGE` or `E_CARD_RO`). The proof uses it with sectors outside the store range and on a card that
/// is not ours; it never targets a sector of a store that is ours.
pub const CMD_CARD_PROBE: u32 = 7;
// Mailbox command 8 was the debug switch (removed: the debug module being loaded is the switch).
/// Test hooks of the settings store (`settings.rs` of the runtime): `arg` = `u8 key_len`, the key, `u8 value_len`, the value (96 bytes in all). `CMD_SETTINGS_PUT` and
/// `CMD_SETTINGS_REMOVE` answer their result; `CMD_SETTINGS_GET` answers the value's length (or a negative code) and leaves the value at the start of `arg`;
/// `CMD_SETTINGS_FLUSH` writes the record now (the runtime writes it 2 s after the last change).
pub const CMD_SETTINGS_PUT: u32 = 9;
pub const CMD_SETTINGS_GET: u32 = 10;
pub const CMD_SETTINGS_REMOVE: u32 = 11;
pub const CMD_SETTINGS_FLUSH: u32 = 12;
