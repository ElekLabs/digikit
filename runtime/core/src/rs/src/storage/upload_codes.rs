//! The first argument of a `logk::UPLOAD` event when it is not a command id (`CMD_*` of the SDK, all below 0x80). One list, so a
//! clash between two events is visible here. `packages/dk-session/src/runtime-events.ts` (`UPLOAD_EVENT`) names the same values.

/// The card lock was busy (`lock`).
pub const LOCK_BUSY: u32 = 0xb5;
/// A pending upload was dropped (`store_writer`).
pub const PENDING_DROPPED: u32 = 0xb6;
/// The +Drive backend reported its reply words (`backends/plusdrive`).
pub const PLUSDRIVE_REPLY: u32 = 0xb7;
/// A write step failed (`store_writer`).
pub const WRITE_FAILED: u32 = 0xb8;
/// CLEAR had no room for the table of the system package and dropped it (`store_cmds`).
pub const CLEAR_DROPPED_PACKAGE: u32 = 0xb9;
