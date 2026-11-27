//! The upload commands about the loaded modules: CAPACITY (the handler only; the reply is built in `capacity.rs`) and the live load, unload and reload (RELOAD, RELOAD_STATE, LOAD, UNLOAD).

use crate::package::resident::store_writer as sw;
use dkrt_abi::mem::wr32;
use crate::package::resident::transport::{rbody, reply};
use core::ptr::addr_of_mut;
use dkrt_sdk::proto::*;
use dkrt_sdk::{LIVE_LOAD, LIVE_RELOAD, LIVE_UNLOAD, OK};

/// Queue a live job (`reload::Job`) for the module at the store path in the body (NUL terminated; the file of a load or a reload is on the card already) and answer `cmd` with its
/// status: 0, `E_MSG` (no path), `E_BUSY` (a job is queued or running), `E_STATE` (an upload is open or a store write is in flight), `E_RELOAD_RETAINED`, `E_NOT_FOUND` (an
/// unload or a reload: no loaded content module or service came from that path), `E_LOADED` (a load: a module of that path is loaded), `E_FORMAT` (a load or an unload of the path of a service: a service is reloaded, with its consumers, and loaded by them).
unsafe fn cmd_live(cmd: u8, op: u32, seq: u32, body: *const u8, blen: usize) -> i32 {
    if blen < 2 || *body.add(blen - 1) != 0 {
        return reply(cmd, seq, E_MSG, 0);
    }
    if sw::is_open() || sw::is_locked() {
        return reply(cmd, seq, E_STATE, 0);
    }
    let r = crate::package::reload::cmd_queue(op, body);
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
    let s = crate::package::reload::state();
    let rb = rbody().add(1);
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

pub(crate) unsafe fn h_capacity(seq: u32, _buf: *mut u8, _body: *const u8, _blen: usize) -> i32 {
    crate::package::capacity::cmd_capacity(seq)
}
