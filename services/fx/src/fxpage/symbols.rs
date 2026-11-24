//! Named statics the hooks of `core/fxpages` read and write (the `DKRT_FXP_*` symbols).

use super::*;
use dkrt_abi::dsp::MAX_FX;

// ---- the state of the hooks: in this file's bss; the asm of the hooks (`fx.dks`) names these symbols, and `bind_table` puts their addresses into `DKRT_FXP_TAB` ----------
/// The table as this service reads it (`info`), and as the hooks read the constants they need.
#[no_mangle]
pub static mut DKRT_FXP_TAB: [u32; TAB_WORDS] = [0; TAB_WORDS];
/// The page rows of the effect pages (`ROW_BYTES` each: name, title, 9 codes), and the send page's row after them.
#[no_mangle]
pub static mut DKRT_FXP_ROWS: [u8; ROW_BYTES * MAX_FX] = [0; ROW_BYTES * MAX_FX];
/// The empty firmware string (`make_rep` of "": length 0, capacity 0, refcount -1, NUL) that the name and title words of a released page row point to.
pub static mut DKRT_FXP_EMPTY: Aligned16 = Aligned16([0; 16]);
#[no_mangle]
pub static mut DKRT_FXP_SEND_ROW: [u8; ROW_BYTES] = [0; ROW_BYTES];
/// The count cell of the track FX key's list (0: the stock list; 2: the send page follows).
#[no_mangle]
pub static mut DKRT_FXP_SEND_COUNT: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_KEY_LIST: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut DKRT_FXP_NSOUND: u32 = 0;
/// The list of page numbers the FX view gets, and its count cell (0: the stock list).
#[no_mangle]
pub static mut DKRT_FXP_LIST_BUF: [u32; LIST_WORDS] = [0; LIST_WORDS];
#[no_mangle]
pub static mut DKRT_FXP_COUNT: u32 = 0;
/// The effect ID of each row and the default raw values (halfwords), written when an effect is registered.
#[no_mangle]
pub static mut DKRT_FXP_IDS: [u16; IDS_MAX] = [0; IDS_MAX];
#[no_mangle]
pub static mut DKRT_FXP_DEFS: [u16; DEFS_MAX] = [0; DEFS_MAX];
/// Counters and pointers of the kit hooks, for tests and the debugger.
#[no_mangle]
pub static mut DKRT_FXP_LAST_LOAD: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_NLOAD: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_NSAVE: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_LAST_SAVE: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_LOAD_SRC: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_SAVE_DST: u32 = 0;
#[no_mangle]
pub static mut DKRT_FXP_NINIT: u32 = 0;
