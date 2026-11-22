//! The module list for the DIGIKIT menu and other UIs: `dkrt_scan` lists the `.dkm` files of the store with what their headers and contents say
//! (accepted by this runtime and device, the needs, loaded), `dkrt_fit` judges a ticked set against the device's limits. The rules are the menu's
//! of before (docs/plans/what-fits.md); the fit rule and its texts are `dkrt-fit`, the needs are `dkneeds`.

use crate::{state::DKRT_PATHS, platform::PLATFORM, platform::device, state::init_once, platform::st, store};
use dkrt_abi::MAX_MODULES;
use dkrt_abi::platform::Tasking;
use core::ptr::{addr_of, addr_of_mut, write_volatile};
use dkneeds::{Bytes, needs_of};
use dkrt_fit as fit;
use dkrt_sdk::*;

/// The files under `modules/` as the store listed them, before the `.dkm` ones are picked.
static mut LIST: [DkrtStoreEntry; SCAN_MAX] = unsafe { core::mem::zeroed() };

/// 512 bytes, 16-byte aligned, in the firmware heap: the block reader's buffer for the header words and the needs. The card DMA fills it,
/// and module RAM stays free of writes by the firmware (the write watch of the memory audit counts DMA too). Allocated once, on the first read.
static mut SECT: *mut u8 = core::ptr::null_mut();

/// The sector reads the last scan made to read the needs.
static mut READS: u32 = 0;

const DKM_MAGIC: u32 = dkneeds::MAGIC;

unsafe fn sect() -> *mut u8 {
    if (*addr_of!(SECT)).is_null() {
        *addr_of_mut!(SECT) = PLATFORM.alloc(512);
    }
    *addr_of!(SECT)
}

/// A header word at byte `off` of the file starting at store sector `first`, read through the block reader; None on an error.
unsafe fn file_word(first: u32, off: u32) -> Option<u32> {
    if store::read_sector(first + off / 512, sect()) != OK {
        return None;
    }
    let p = sect().add((off % 512) as usize);
    Some(
        ((*p as u32) << 24)
            | ((*p.add(1) as u32) << 16)
            | ((*p.add(2) as u32) << 8)
            | (*p.add(3) as u32),
    )
}

/// `dkrt_module_header(first, out)`: the header fields of the `.dkm` at store sector `first` (the version, header 0x34), read by one sector read.
pub unsafe fn module_header(first: u32, out: *mut DkrtModuleHeader) -> i32 {
    let r = store::read_sector(first, sect());
    if r != OK {
        return r;
    }
    let rd = |off: usize| -> u32 {
        let p = sect().add(off);
        ((*p as u32) << 24) | ((*p.add(1) as u32) << 16) | ((*p.add(2) as u32) << 8) | (*p.add(3) as u32)
    };
    if rd(dkneeds::H_MAGIC as usize) != DKM_MAGIC {
        return E_FORMAT;
    }
    (*out).version = rd(dkneeds::H_VERSION as usize);
    OK
}

/// Whether the file at store sector `first` can be loaded by this runtime on this device, judged from its header: the runtime version the module
/// needs (header word 0x30, the loader's rule `runtime_accepts`), and the device of its DSP section (a module without one runs on either device).
/// A file that is not a `.dkm` or cannot be read passes: the runtime then names the real reason at boot.
unsafe fn compatible(first: u32, device: u32) -> bool {
    match file_word(first, dkneeds::H_MAGIC) {
        Some(DKM_MAGIC) => {}
        _ => return true,
    }
    if !file_word(first, dkneeds::H_RUNTIME).is_none_or(crate::state::runtime_accepts) {
        return false;
    }
    let (Some(off), Some(size)) = (file_word(first, dkneeds::H_DSP_OFF), file_word(first, dkneeds::H_DSP_SIZE)) else {
        return true;
    };
    if size == 0 {
        return true;
    }
    match file_word(first, off.wrapping_add(dkneeds::DSP_H_DEVICE)) {
        Some(d) => d == device || device == 0,
        None => true,
    }
}

/// Is the file at store sector `first` a `.dkm` packed with `--trace` (bit 31 of the word at 0x30, which the loader ignores)?
unsafe fn traced(first: u32) -> bool {
    file_word(first, dkneeds::H_MAGIC) == Some(DKM_MAGIC) && file_word(first, dkneeds::H_RUNTIME).is_some_and(|w| w >> 31 != 0)
}

/// A card file as the needs reader's byte source: the store's sector reads, one sector cached (the file is never staged).
struct CardFile {
    first: u32,
    size: u32,
    /// the file's sector held in `SECT`
    held: u32,
}

impl Bytes for CardFile {
    fn size(&self) -> u32 {
        self.size
    }
    fn read(&mut self, at: u32, out: &mut [u8]) -> bool {
        if at > self.size || out.len() as u32 > self.size - at {
            return false;
        }
        let mut i = 0usize;
        while i < out.len() {
            let pos = at + i as u32;
            let sector = pos / 512;
            if self.held != sector {
                // `held` is not trusted across a failed read: the block reader may have written part of the buffer
                self.held = u32::MAX;
                unsafe {
                    write_volatile(addr_of_mut!(READS), READS + 1);
                    if store::read_sector(self.first + sector, sect()) != OK {
                        return false;
                    }
                    out[i] = *sect().add((pos % 512) as usize);
                }
                self.held = sector;
            } else {
                out[i] = unsafe { *sect().add((pos % 512) as usize) };
            }
            i += 1;
        }
        true
    }
}

fn is_dkm(p: &[u8; 64]) -> bool {
    let mut l = 0;
    while l < 63 && p[l] != 0 {
        l += 1;
    }
    let lower = |c: u8| if c.is_ascii_uppercase() { c + 32 } else { c };
    l > 4
        && p[l - 4] == b'.'
        && lower(p[l - 3]) == b'd'
        && lower(p[l - 2]) == b'k'
        && lower(p[l - 1]) == b'm'
}

fn same_path(a: &[u8; 64], b: &[u8; 64]) -> bool {
    let mut i = 0;
    while i < 63 {
        if a[i] != b[i] {
            return false;
        }
        if a[i] == 0 {
            return true;
        }
        i += 1;
    }
    true
}

/// `dkrt_scan(out, max)`: the `.dkm` files under `modules/` in path order, at most `max` (and `SCAN_MAX`) into `out`; the count, or a negative code
/// (`E_NOT_FOUND`: no store). A commit of an upload that ran during the list makes it start again inside the store (finding 43, "the menu's reader and
/// a commit"); the header words read after belong to the same table or fail the load check. UI task.
pub unsafe extern "C" fn dkrt_scan(out: *mut DkrtScan, max: u32) -> i32 {
    init_once();
    let list = addr_of_mut!(LIST) as *mut DkrtStoreEntry;
    let n = store::list(b"modules/\0".as_ptr(), list, SCAN_MAX as u32);
    if n < 0 {
        return n;
    }
    let dev = device();
    write_volatile(addr_of_mut!(READS), 0);
    let mut count = 0usize;
    for e in 0..n as usize {
        let le = &*list.add(e);
        if !is_dkm(&le.path) || count >= max as usize || count >= SCAN_MAX {
            continue;
        }
        let o = &mut *out.add(count);
        *o = core::mem::zeroed();
        o.path = le.path;
        o.first = le.first;
        o.size = le.size;
        if traced(le.first) {
            o.flags |= SCAN_TRACE;
        }
        if compatible(le.first, dev) {
            o.flags |= SCAN_COMPAT;
            let mut f = CardFile {
                first: le.first,
                size: le.size,
                held: u32::MAX,
            };
            if let Ok(nd) = needs_of(&mut f) {
                fit::set_scan_needs(o, &nd);
            }
        }
        count += 1;
    }
    // loaded: the runtime's modules, matched by the store path they came from
    let s = st();
    let mut m = 0;
    while m < MAX_MODULES {
        if s.modules[m].used != 0 {
            let path = &(*addr_of!(DKRT_PATHS))[m];
            let mut i = 0;
            while i < count {
                let o = &mut *out.add(i);
                if same_path(&o.path, path) {
                    o.flags |= SCAN_LOADED;
                    if s.modules[m].used == 2 {
                        o.flags |= SCAN_LOADING;
                    }
                }
                i += 1;
            }
        }
        m += 1;
    }
    count as i32
}

/// `CoreApi::module_path`: the store path of the content module in record `owner - 1`, null for a record with no loaded module (`used` is 1 loaded, 2 loading).
pub unsafe fn module_path(owner: u8) -> *const u8 {
    let rec = owner as usize;
    if rec == 0 || rec > MAX_MODULES || st().modules[rec - 1].used == 0 {
        return core::ptr::null();
    }
    (*addr_of!(DKRT_PATHS))[rec - 1].as_ptr()
}

/// `CoreApi::store_find`: the first file under `dir` (at most 62 bytes, ending in `/`) whose last path part is `name`, in the table's order. The length of its path,
/// with the path NUL padded in `out`; `E_NOT_FOUND` after the whole store was read and holds none (and without a store), the error of the read otherwise (`E_BUSY`
/// while an upload holds the store). UI task.
pub unsafe fn store_find(dir: &[u8], name: &[u8], out: &mut [u8; 64]) -> i32 {
    if dir.len() > 62 {
        return E_NOT_FOUND;
    }
    let mut prefix = [0u8; 64];
    prefix[..dir.len()].copy_from_slice(dir);
    store::find_name(prefix.as_ptr(), name, out)
}

/// `dkrt_scan_reads()`: the sector reads the last `dkrt_scan` made to read the needs.
pub unsafe extern "C" fn dkrt_scan_reads() -> u32 {
    core::ptr::read_volatile(addr_of!(READS))
}

/// `dkrt_fit(entries, n, ticked, out)`: the verdict per entry and the load line of the ticked set (`dkrt_fit::fit_set`). 0.
pub unsafe extern "C" fn dkrt_fit(
    entries: *const DkrtScan,
    n: u32,
    ticked: *const u8,
    out: *mut DkrtFitOut,
) -> i32 {
    init_once();
    let n = if n as usize > SCAN_MAX {
        SCAN_MAX
    } else {
        n as usize
    };
    let mut caps = [DkrtCap {
        kind: 0,
        limit: 0,
        used: 0,
        largest: 0,
    }; CAP_MAX];
    let k = crate::package::capacity::dkrt_capacity(caps.as_mut_ptr(), CAP_MAX as u32) as usize;
    let (e, t): (&[DkrtScan], &[u8]) = if n == 0 {
        (&[], &[])
    } else {
        (
            core::slice::from_raw_parts(entries, n),
            core::slice::from_raw_parts(ticked, n),
        )
    };
    fit::fit_set(&mut caps[..k], e, t, &mut *out);
    OK
}

#[cfg(test)]
mod find_tests {
    use super::*;

    #[test]
    fn only_a_module_record_that_is_in_use_has_a_path() {
        let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let saved = *addr_of!(DKRT_PATHS);
            (&mut (*addr_of_mut!(DKRT_PATHS))[1])[..9].copy_from_slice(b"modules/a");
            st().modules[1].used = 0;
            assert!(module_path(2).is_null(), "a record that holds no module");
            st().modules[1].used = 1;
            let p = module_path(2);
            assert!(!p.is_null());
            assert_eq!(core::slice::from_raw_parts(p, 9), b"modules/a");
            st().modules[1].used = 2;
            assert!(!module_path(2).is_null(), "a module whose DSP part still loads is loaded");
            assert!(module_path(0).is_null(), "owner 0 is no record");
            assert!(module_path((MAX_MODULES + 1) as u8).is_null(), "a service file's record is no content module");
            st().modules[1].used = 0;
            *addr_of_mut!(DKRT_PATHS) = saved;
        }
    }

}
