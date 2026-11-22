//! The upload commands about the runtime itself: INFO, CAPACITY, SETTINGS, FAULT and RESTART (with the pending restart that `restart_poll` runs).

use dkrt_abi::platform::Platform;
use crate::store_writer::{self as sw};
use crate::store_writer::{copy, free_bytes};
use crate::platform::{rd32, tx_in_flight, wr32};
use crate::{boot, card, dlog};
use core::ptr::addr_of_mut;
use dkrt_sdk::proto::*;
use dkrt_sdk::{logk, DkrtCap, CAP_MAX, LIVE_LOAD, LIVE_RELOAD, LIVE_UNLOAD, OK, RUNTIME_VERSION};
use crate::transport::*;
use crate::transport::MAX_CHUNK;
use crate::store_cmds::state;

pub(crate) unsafe fn cmd_info(seq: u32, buf: *mut u8) -> i32 {
    let st = state(buf);
    let free = match &st {
        Ok(s) => free_bytes(&s.h),
        Err(_) => 0,
    };
    let rb = (addr_of_mut!(RBODY) as *mut u8).add(1);
    *rb = 0;
    *rb.add(1) = VERSION; // protocol 1
    *rb.add(2) = (RUNTIME_VERSION >> 8) as u8;
    *rb.add(3) = RUNTIME_VERSION as u8;
    wr32(rb.add(4), crate::platform::device_name()); // the device's short name from its device file
    put32(1 + 8, free);
    put32(1 + 12, MAX_CHUNK);
    let flags = (boot::info().safe & 1)
        | if sw::is_open() { 2 } else { 0 }
        | if card::writable() { 0 } else { 4 }
        | if dlog::sink_present() { 8 } else { 0 }
        | if crate::fault::seen().found != 0 { 16 } else { 0 };
    *rb.add(16) = flags as u8;
    // the log range (bytes 17 to 24) stays zero: the log is the debug module's, whose LOG command has the range
    wr32(rb.add(17), 0);
    wr32(rb.add(21), 0);
    // appended after the log range: the whole packed runtime version (bytes 2-3 keep its low 16 bits) and the capability bits
    wr32(rb.add(25), RUNTIME_VERSION);
    // the firmware identity of the image (the key of the port file), when the image has it: INFO ends with it
    let fw_id = crate::loader::role::image_fw_id(&crate::platform::RealPlatform);
    let fw_cap = if fw_id.is_some() { CAP_FW_ID } else { 0 };
    let log_caps = if dlog::sink_present() { CAP_LOG | CAP_PEEK | CAP_LOGCFG } else { 0 };
    wr32(rb.add(29), log_caps | CAP_SELECT | CAP_RESTART | CAP_MODULES | CAP_CAPACITY | CAP_READ | CAP_VERIFY | CAP_SERVICE_CMDS | CAP_FOLDERS | CAP_SETTINGS | CAP_FILEHASH | CAP_RENAME | CAP_READBACK | CAP_LOADS | CAP_SVC_ABI | CAP_RELOAD | CAP_LIVE | fw_cap);
    // appended after the capability bits: the id of the flashed image (patchkit stamps it; zeros when there is none)
    let id = crate::platform::image_id();
    let mut i = 0;
    while i < 8 {
        *rb.add(33 + i) = id[i];
        i += 1;
    }
    // appended after the image id: the ABI fingerprint of interfaces B and C (`dkrt_abi::SVC_ABI`, CAP_SVC_ABI), the word a service file carries at header 0x3c
    wr32(rb.add(41), dkrt_abi::service::SVC_ABI);
    // appended after the ABI fingerprint, with CAP_FW_ID: the firmware identity (8 bytes), which names the port file of this image
    let mut n = 45;
    if let Some(id) = fw_id {
        let mut i = 0;
        while i < 8 {
            *rb.add(45 + i) = id[i];
            i += 1;
        }
        n = 53;
    }
    // then the card's capacity in sectors (0xFFFFFFFF: the driver is not ready) and the card check's state (0 unknown, 1 ours, 2 blank, 3 foreign, 4 unreadable,
    // 5 too small); a client that stops earlier ignores them
    wr32(rb.add(n), crate::platform::PLATFORM.card_sectors());
    wr32(rb.add(n + 4), card::state());
    // then the card write guard's counters (core/card-guard): `u32 version` (0: the module is not in the image; the other words are then 0), refused writes, refused erases,
    // and the last refused request (first sector, length in sectors, class | kind << 8; class 0 stock, 1 runtime; kind 0 write, 1 erase)
    let g = crate::storage::card_guard::stats(&crate::platform::PLATFORM).words();
    let mut k = 0;
    while k < 6 {
        wr32(rb.add(n + 8 + 4 * k), g[k]);
        k += 1;
    }
    reply(CMD_INFO, seq, OK, n + 8 + 24)
}

/// CAPACITY: per resource of the runtime, its limit, what is used and the largest free piece. Body: none. Reply: `u8 version` (1), `u8 n`, then
/// `n` entries of `u8 kind` (`CAPK_*`, sdk/sdk.toml `[capacity.kinds]`), `u32 limit`, `u32 used`, `u32 largest` (0 where a largest piece has
/// no meaning). A host skips a kind it does not know; new kinds are appended.
pub(crate) unsafe fn cmd_capacity(seq: u32) -> i32 {
    let mut entries = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
    let n = crate::report::dkrt_capacity(entries.as_mut_ptr(), CAP_MAX as u32) as usize;
    let rb = (addr_of_mut!(RBODY) as *mut u8).add(1);
    *rb = 1;
    *rb.add(1) = n as u8;
    let mut at = 2usize;
    let mut k = 0;
    while k < n {
        let e = &entries[k];
        *rb.add(at) = e.kind as u8;
        wr32(rb.add(at + 1), e.limit);
        wr32(rb.add(at + 5), e.used);
        wr32(rb.add(at + 9), e.largest);
        at += 13;
        k += 1;
    }
    reply(CMD_CAPACITY, seq, OK, at)
}

/// SETTINGS (read only): the settings record as the runtime holds it (docs/design/digikit-store.md, "The settings record"). Body: `u32 offset`, `u16 length`
/// (1 to 768). Reply: `u8 flags` (bit 0: the record has changes that the card does not hold yet), `u8 copy` (the card's current copy: 0 A, 1 B, 2 none),
/// `u32 seq` (the card's), `u32 used` (the record's bytes), then `min(length, used - offset)` bytes of the record from `offset`. A host reads in pieces
/// and decodes the whole (the format is `dkrt_sdk::settings`). E_SET_NOKEY: no record on the card and none made; E_OFFSET: offset past the record.
pub(crate) unsafe fn cmd_settings(seq: u32, body: *const u8, blen: usize) -> i32 {
    if blen != 6 {
        return reply(CMD_SETTINGS, seq, E_MSG, 0);
    }
    let off = rd32(body) as usize;
    let mut len = (((*body.add(4) as u32) << 8) | *body.add(5) as u32) as usize;
    if len == 0 {
        return reply(CMD_SETTINGS, seq, E_LENGTH, 0);
    }
    if crate::settings::is_off() {
        return reply(CMD_SETTINGS, seq, E_SET_OFF, 0);
    }
    let (flags, cur, rseq, rec) = match crate::settings::snapshot() {
        Some(r) => r,
        None => return reply(CMD_SETTINGS, seq, E_SET_NOKEY, 0),
    };
    if off >= rec.len() {
        return reply(CMD_SETTINGS, seq, E_OFFSET, 0);
    }
    if len > READ_MAX as usize {
        len = READ_MAX as usize;
    }
    if len > rec.len() - off {
        len = rec.len() - off;
    }
    let rb = (addr_of_mut!(RBODY) as *mut u8).add(1);
    *rb = flags;
    *rb.add(1) = cur as u8;
    wr32(rb.add(2), rseq);
    wr32(rb.add(6), rec.len() as u32);
    copy(rb.add(10), rec.as_ptr().add(off), len);
    reply(CMD_SETTINGS, seq, OK, 10 + len)
}

/// FAULT (dev): body `u8 what`; 1 makes the runtime panic with a failed bounds check, to test the fault path. It does not reply: the device resets.
/// Other values: `E_PROTO`.
pub(crate) unsafe fn cmd_fault(seq: u32, body: *const u8, blen: usize) -> i32 {
    if blen != 1 {
        return reply(CMD_FAULT, seq, E_MSG, 0);
    }
    if *body != 1 {
        return reply(CMD_FAULT, seq, E_PROTO, 0);
    }
    let a = [1u8, 2, 3, 4];
    let i = core::hint::black_box(a.len() + 3);
    reply(CMD_FAULT, seq, OK, a[i] as usize)
}

/// Ticks to wait for the reply to leave the TX ring, at most, before the restart goes anyway.
pub(crate) const RESTART_TICKS_MAX: u32 = 4000;
/// 0: no restart pending; else 1 + the UI passes waited so far.
pub(crate) static mut RESTART_WAIT: u32 = 0;

/// RESTART (runtime 4): the reply goes out first; the soft reset (`dkrt_restart`, the one APPLY & RESTART calls) runs from the UI task's next
/// passes (`restart_poll`), once the TX ring has drained. Status: 0, or `E_STATE` while an upload is open. No body.
pub(crate) unsafe fn cmd_restart(seq: u32) -> i32 {
    // an upload is open (an SDK commit keeps its session open until it is done), or a store operation is in flight
    if sw::is_open() || sw::is_locked() {
        return reply(CMD_RESTART, seq, E_STATE, 0);
    }
    RESTART_WAIT = 1;
    reply(CMD_RESTART, seq, OK, 0)
}

/// Called once per UI pass (`dkrt_tick`). Runs a pending RESTART after its reply has been sent. `dkrt_restart` returns only when it refuses
/// (a card transfer in flight): the request is then dropped and logged as an error; the host sees the runtime still running and asks again.
pub unsafe fn restart_poll() {
    if RESTART_WAIT == 0 {
        return;
    }
    if sw::is_locked() {
        return; // a store write is in flight: the reset waits (the wait counter does not run)
    }
    RESTART_WAIT += 1;
    let drained = matches!(tx_in_flight(), Some(0));
    if (drained && RESTART_WAIT > 3) || RESTART_WAIT > RESTART_TICKS_MAX {
        RESTART_WAIT = 0;
        let r = crate::api::dkrt_restart();
        dlog::ev(logk::UPLOAD, logk::F_ERROR, [CMD_RESTART as u32, r as u32, 0, 0, 0]);
    }
}

/// Queue a live job (`reload::Job`) for the module at the store path in the body (NUL terminated; the file of a load or a reload is on the card already) and answer `cmd` with its
/// status: 0, `E_MSG` (no path), `E_BUSY` (a job is queued or running), `E_STATE` (an upload is open or a store write is in flight), `E_RELOAD_RETAINED`, `E_NOT_FOUND` (an
/// unload or a reload: no loaded content module came from that path), `E_LOADED` (a load: a module of that path is loaded), `E_FORMAT` (the path is the file of a service).
unsafe fn cmd_live(cmd: u8, op: u32, seq: u32, body: *const u8, blen: usize) -> i32 {
    if blen < 2 || *body.add(blen - 1) != 0 {
        return reply(cmd, seq, E_MSG, 0);
    }
    if sw::is_open() || sw::is_locked() {
        return reply(cmd, seq, E_STATE, 0);
    }
    let r = crate::reload::cmd_queue(op, body);
    reply(cmd, seq, r, 0)
}

/// RELOAD (runtime 4): queue the reload of the module that was loaded from the store path in the body. The job runs from the UI task's passes (`reload::poll`);
/// `RELOAD_STATE` says where it is and how it ended.
pub(crate) unsafe fn cmd_reload(seq: u32, body: *const u8, blen: usize) -> i32 {
    cmd_live(CMD_RELOAD, LIVE_RELOAD, seq, body, blen)
}

/// LOAD (runtime 5): queue the load of the module file at the store path in the body (no loaded module came from it).
pub(crate) unsafe fn cmd_load(seq: u32, body: *const u8, blen: usize) -> i32 {
    cmd_live(CMD_LOAD, LIVE_LOAD, seq, body, blen)
}

/// UNLOAD (runtime 5): queue the unload of the module that was loaded from the store path in the body.
pub(crate) unsafe fn cmd_unload(seq: u32, body: *const u8, blen: usize) -> i32 {
    cmd_live(CMD_UNLOAD, LIVE_UNLOAD, seq, body, blen)
}

/// RELOAD_STATE: `u8 phase` (0 none yet, 1 queued or running, 2 finished), `i32 result`, `u8 step`, `u8 unloaded` (1: the old module was torn down, so it is not loaded
/// unless the result is 0 and the job loaded; 0: a refusal or an abort left it running), `u8 op` (1 load, 2 unload, 3 reload of the last job; 0 none yet). The state of the one live job, whichever operation it is. No body.
pub(crate) unsafe fn cmd_reload_state(seq: u32) -> i32 {
    let s = crate::reload::state();
    let rb = (addr_of_mut!(RBODY) as *mut u8).add(1);
    *rb = s.phase;
    wr32(rb.add(1), s.result as u32);
    *rb.add(5) = s.step;
    *rb.add(6) = s.torn as u8;
    *rb.add(7) = s.op;
    reply(CMD_RELOAD_STATE, seq, OK, 8)
}

pub(crate) unsafe fn h_reload(seq: u32, _buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    cmd_reload(seq, body, blen)
}
pub(crate) unsafe fn h_load(seq: u32, _buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    cmd_load(seq, body, blen)
}
pub(crate) unsafe fn h_unload(seq: u32, _buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    cmd_unload(seq, body, blen)
}
pub(crate) unsafe fn h_reload_state(seq: u32, _buf: *mut u8, _body: *const u8, _blen: usize) -> i32 {
    cmd_reload_state(seq)
}
pub(crate) unsafe fn h_info(seq: u32, buf: *mut u8, _body: *const u8, _blen: usize) -> i32 {
    cmd_info(seq, buf)
}
pub(crate) unsafe fn h_capacity(seq: u32, _buf: *mut u8, _body: *const u8, _blen: usize) -> i32 {
    cmd_capacity(seq)
}
pub(crate) unsafe fn h_settings(seq: u32, _buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    cmd_settings(seq, body, blen)
}
pub(crate) unsafe fn h_fault(seq: u32, _buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    cmd_fault(seq, body, blen)
}
pub(crate) unsafe fn h_restart(seq: u32, _buf: *mut u8, _body: *const u8, _blen: usize) -> i32 {
    cmd_restart(seq)
}

