//! The layout check of a `register` section, shared by the runtime's registry walker and the service contract suite (`dkrt-test-support`). It is not a boundary file
//! of the service ABI (`SVC_ABI_FILES`): the services call nothing of it.
use crate::mem::rd32;
use dkrt_sdk::{E_FORMAT, E_REG_DEVICE, OK};

const REG_MAGIC: u32 = 0x444b_5247; // "DKRG"
const MAX_RECORDS: u32 = dkneeds::MAX_RECORDS;

/// The layout check of a `register` section of `size` bytes (not 0): the header, the device, the number of records, and for each record a kind with a length
/// (`len_of(kind)`: `None` for a kind no service declared) and that length exactly, up to the end of the section. Nothing is written. The registry walker of the
/// runtime and the service contract suite both call it, so the suite checks what the loader checks.
pub unsafe fn check_layout(
    reg: *const u8,
    size: u32,
    device: u32,
    len_of: impl Fn(u32) -> Option<u32>,
) -> i32 {
    if size < 16 || rd32(reg) != REG_MAGIC || rd32(reg.add(4)) != 1 {
        return E_FORMAT;
    }
    if rd32(reg.add(8)) != device {
        return E_REG_DEVICE;
    }
    let n = rd32(reg.add(12));
    if n > MAX_RECORDS {
        return E_FORMAT;
    }
    let mut at: u32 = 16;
    let mut i = 0;
    while i < n {
        if at > size || size - at < 8 {
            return E_FORMAT;
        }
        let kind = rd32(reg.add(at as usize));
        let len = rd32(reg.add(at as usize + 4));
        let want = match len_of(kind) {
            Some(want) => want,
            None => return E_FORMAT,
        };
        if len != want || size - at < len {
            return E_FORMAT;
        }
        at += len;
        i += 1;
    }
    if at != size {
        return E_FORMAT;
    }
    OK
}
