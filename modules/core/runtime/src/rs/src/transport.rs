//! Module upload over USB SysEx (docs/design/digikit-upload.md): the device side.
//!
//! `dkrt_midi_rx` is called from the firmware's MIDI task (hook `cf.midi.site.rx`, `src/cf/tramp.s`) with every SysEx message that
//! came in on USB cable 0, after the firmware gathered it and before its router. A message that starts `F0 7D 64 6B` is ours: it is
//! handled here (one request, one reply, in order) and the router never sees it; any other message returns 0 and takes the stock route.
//!
//! A reply is sent only when the firmware's TX ring has room (`tx_room`), so a host that does not read its input cannot hang the MIDI task.
//!
//! The store write steps (begin, write, close, abort, delete, the session, the store lock, the card write and its test knobs) are in
//! `store_writer.rs`; the handlers here decode a request, call them and reply. A command that reads or writes the store holds the store
//! lock for its length; when an SDK call holds it, the reply is `E_BUSY` and the host asks again. Rules: no `match` on integers (a dense one becomes a jump
//! table, which is not ColdFire code), no `core::fmt`, no slice indexing (a bounds panic hangs the task).
use crate::store_writer::{self as sw, SCRATCH_AT};
use crate::store_writer::{copy, DKRT_UPLOAD_WRITES};
use crate::platform::wr32;
use crate::PLATFORM;
use dkrt_abi::platform::Platform;
use crate::dlog;
use core::ptr::addr_of_mut;
use dkrt_sdk::proto::*;
use dkrt_sdk::{logk, E_BUSY, OK};
use crate::module_cmds::{service, service_of};
use crate::store_cmds::*;
use crate::admin_cmds::*;
use crate::module_cmds::*;


// The protocol numbers (commands, the `E_*` statuses, `VERSION`, the INFO capability bits) are `dkrt_sdk::proto`, written by `patchkit gen`
// from `[protocol]` of `sdk/sdk.toml`. E_INUSE: the card's store area is not ours and not blank, the runtime writes nothing there
// (finding 46). E_PEEK: the range is not inside a range the device file lists, or the length is 0 or over 512.

/// Largest DATA chunk (INFO says so) and the unpacked body we accept: offset (4) + chunk.
/// Largest DATA chunk (INFO says so) and the unpacked body we accept: offset (4) + chunk.
pub const MAX_CHUNK: u32 = dkrt_sdk::proto::MAX_CHUNK as u32;
/// The unpacked request body starts here in the buffer, so that a DATA chunk (after its 4-byte offset) is 16-byte aligned.
pub(crate) const BODY_AT: usize = 12;
pub(crate) const BODY_MAX: usize = 4 + MAX_CHUNK as usize;
/// The buffer: 32 KiB of body room, then a scratch region (table read-back, header sector).
pub(crate) const SCRATCH_BYTES: usize = 4096;
pub(crate) const BUF_BYTES: usize = SCRATCH_AT + SCRATCH_BYTES;

/// The buffer: static (bss of this object, so patchkit reserves it with the code), 16-byte aligned. A static address lets the write-watch
/// of the proofs leave out the card reads that land here by DMA, as it does for `DKRT_STAGE`.
#[repr(C, align(16))]
pub struct UploadBuf([u8; BUF_BYTES]);
#[no_mangle]
pub static mut DKRT_UPLOAD_BUF: UploadBuf = UploadBuf([0; BUF_BYTES]);
/// Wire bytes of a reply are at most 1 KiB: the unpacked body at most this.
pub(crate) const REPLY_BODY_MAX: usize = 880;
pub(crate) const REQ_WIRE_MAX: u32 = dkrt_sdk::proto::MAX_REQUEST_WIRE as u32;
/// A request body that is not DATA or SELECT, copied out of the buffer (a store path is up to 159 bytes: BEGIN's body is at most 196).
pub(crate) static mut REQ: [u8; 256] = [0; 256];
/// Reply under construction: the unpacked body, then the wire message.
pub(crate) static mut RBODY: [u8; REPLY_BODY_MAX] = [0; REPLY_BODY_MAX];
pub(crate) static mut RWIRE: [u8; 1100] = [0; 1100];
/// Replies dropped because the TX ring had no room (the host retries).
#[no_mangle]
pub static mut DKRT_UPLOAD_DROPPED: u32 = 0;
/// Requests handled and the last status (the proofs peek these).
#[no_mangle]
pub static mut DKRT_UPLOAD_COUNT: u32 = 0;
#[no_mangle]
pub static mut DKRT_UPLOAD_LAST: i32 = 0;

// ---- 8-in-7 --------------------------------------------------------------------------------------------------------------

/// Unpack `n` wire bytes into `dst`; returns the unpacked length. Group: a collector byte, then up to 7 bytes; byte j (1..7) takes
/// bit 7 from collector bit (7 - j).
pub(crate) unsafe fn unpack(src: *const u8, n: usize, dst: *mut u8) -> usize {
    let mut i = 0;
    let mut o = 0;
    // whole groups (a collector and seven bytes) with the shifts as constants: about a third of the instructions of the loop below,
    // which every byte of a DATA chunk went through
    while i + 8 <= n {
        let c = *src.add(i) as u32;
        let s = src.add(i + 1);
        let d = dst.add(o);
        *d = (*s & 0x7f) | (((c << 1) & 0x80) as u8);
        *d.add(1) = (*s.add(1) & 0x7f) | (((c << 2) & 0x80) as u8);
        *d.add(2) = (*s.add(2) & 0x7f) | (((c << 3) & 0x80) as u8);
        *d.add(3) = (*s.add(3) & 0x7f) | (((c << 4) & 0x80) as u8);
        *d.add(4) = (*s.add(4) & 0x7f) | (((c << 5) & 0x80) as u8);
        *d.add(5) = (*s.add(5) & 0x7f) | (((c << 6) & 0x80) as u8);
        *d.add(6) = (*s.add(6) & 0x7f) | (((c << 7) & 0x80) as u8);
        i += 8;
        o += 7;
    }
    // the last, short group
    while i < n {
        let c = *src.add(i);
        i += 1;
        let mut j = 1;
        while j <= 7 && i < n {
            let b = *src.add(i) & 0x7f;
            *dst.add(o) = b | (((c >> (7 - j)) & 1) << 7);
            o += 1;
            i += 1;
            j += 1;
        }
    }
    o
}

/// Unpacked length of `n` wire bytes.
pub(crate) fn unpacked_len(n: usize) -> usize {
    n - (n + 7) / 8
}

/// Pack `n` bytes into `dst`; returns the wire length.
pub(crate) unsafe fn pack(src: *const u8, n: usize, dst: *mut u8) -> usize {
    let mut i = 0;
    let mut o = 0;
    while i < n {
        let c_at = o;
        o += 1;
        let mut c = 0u8;
        let mut j = 1;
        while j <= 7 && i < n {
            let b = *src.add(i);
            c |= (b >> 7) << (7 - j);
            *dst.add(o) = b & 0x7f;
            o += 1;
            i += 1;
            j += 1;
        }
        *dst.add(c_at) = c;
    }
    o
}

// ---- replies -------------------------------------------------------------------------------------------------------------

/// The most bytes the firmware may hold unsent in its USB MIDI TX ring (4 KiB) when a reply is sent. The TX function spins, with the MIDI
/// task in it, while the bytes queued plus this word exceed 0xffb, and the USB stack has 31 transfer buffers that a send takes one of (and
/// waits for when none is free): our smallest reply is a 16-byte transfer, so 256 bytes cap the replies waiting at 16 and leave the
/// stock traffic (clock, notes: 4 bytes a transfer) its share. Above this the reply is dropped.
pub(crate) const TX_BUSY_MAX: u32 = 256;
/// The ring's size less its own margin (the function's `0xffb`).
pub(crate) const TX_RING: u32 = 0xff0;

/// Is there room to send a reply of `wire` bytes without the firmware's TX function waiting? It packs 3 bytes of a SysEx message into
/// each 4-byte event, and waits only when the bytes in flight plus what it queued reach the ring size (finding 43).
pub(crate) unsafe fn tx_room(wire: usize) -> bool {
    tx_room_with(&PLATFORM, wire)
}

/// `tx_room` over a platform.
pub(crate) unsafe fn tx_room_with<P: Platform>(p: &P, wire: usize) -> bool {
    let need = ((wire as u32 + 2) / 3 + 1) * 4;
    match p.midi_tx_in_flight() {
        Some(f) => f <= TX_BUSY_MAX && f + need <= TX_RING,
        None => false, // the anchor does not point at a counter: no reply rather than a possible hang
    }
}

/// Send the reply for `cmd`/`seq`: status, then the `extra` bytes already in `RBODY[1..1+extra]`.
pub(crate) unsafe fn reply(cmd: u8, seq: u32, status: i32, extra: usize) -> i32 {
    reply_with(&PLATFORM, cmd, seq, status, extra)
}

/// `reply` over a platform (the host tests capture the MIDI output with `FakePlatform`).
pub(crate) unsafe fn reply_with<P: Platform>(p: &P, cmd: u8, seq: u32, status: i32, extra: usize) -> i32 {
    let rb = addr_of_mut!(RBODY) as *mut u8;
    *rb = status as u8;
    let w = addr_of_mut!(RWIRE) as *mut u8;
    *w = 0xf0;
    *w.add(1) = 0x7d;
    *w.add(2) = 0x64;
    *w.add(3) = 0x6b;
    *w.add(4) = VERSION;
    *w.add(5) = (cmd | 0x40) & 0x7f;
    *w.add(6) = (seq & 0x7f) as u8;
    let n = pack(rb, 1 + extra, w.add(7));
    *w.add(7 + n) = 0xf7;
    DKRT_UPLOAD_LAST = status;
    if logged(cmd, status) {
        dlog::ev(logk::UPLOAD, if status == 0 { dlog::INFO } else { logk::F_ERROR }, [cmd as u32, status as u32, DKRT_UPLOAD_WRITES, 0, 0]);
    }
    if !tx_room_with(p, 8 + n) {
        DKRT_UPLOAD_DROPPED = DKRT_UPLOAD_DROPPED.wrapping_add(1);
        return 1; // the host reads nothing, or slowly: its retry asks again
    }
    p.midi_send(2, core::slice::from_raw_parts(w, 8 + n));
    DKRT_UPLOAD_COUNT = DKRT_UPLOAD_COUNT.wrapping_add(1);
    1
}

pub(crate) unsafe fn put32(at: usize, v: u32) {
    wr32((addr_of_mut!(RBODY) as *mut u8).add(at), v);
}

pub(crate) unsafe fn buffer() -> *mut u8 {
    addr_of_mut!(DKRT_UPLOAD_BUF) as *mut u8
}

/// The hook: `msg` is one gathered SysEx message, F0 to F7 inclusive. Returns 1 when it was ours (the router must not see it).
#[no_mangle]
pub unsafe extern "C" fn dkrt_midi_rx(msg: *const u8, len: u32, _source: u32) -> i32 {
    sw::note_midi_task();
    if len < 5 || *msg != 0xf0 || *msg.add(1) != 0x7d || *msg.add(2) != 0x64 || *msg.add(3) != 0x6b {
        return 0;
    }
    crate::init_once();
    if len < 8 || *msg.add(len as usize - 1) != 0xf7 {
        return reply(0, 0, E_MSG, 0);
    }
    let ver = *msg.add(4);
    let cmd = *msg.add(5);
    let seq = *msg.add(6) as u32;
    if ver != VERSION {
        return reply(cmd, seq, E_PROTO, 0);
    }
    let wire = len - 8;
    if wire > REQ_WIRE_MAX || unpacked_len(wire as usize) > BODY_MAX {
        return reply(cmd, seq, E_MSG, 0);
    }
    let buf = buffer();
    let blen = unpack(msg.add(7), wire as usize, buf.add(BODY_AT));
    let ent = find(cmd);
    let big = ent.is_some_and(|e| e.flags & BIG != 0);
    // Every command but DATA and SELECT has a small body. Copy it out: the commands stream the table through the buffer.
    // (DATA and SELECT read theirs in place: SELECT's is up to 8 paths of 56 bytes and it never touches the table; DATA's is a chunk.)
    let req = &mut *addr_of_mut!(REQ);
    if !big {
        if blen > req.len() {
            return reply(cmd, seq, E_MSG, 0);
        }
        copy(req.as_mut_ptr(), buf.add(BODY_AT), blen);
    }
    let body = if big { buf.add(BODY_AT) as *const u8 } else { req.as_ptr() };
    if let Some(e) = ent {
        if e.flags & STORE == 0 {
            return (e.h)(seq, buf, body, blen);
        }
        if !sw::try_lock() {
            return reply(cmd, seq, E_BUSY, 0); // an SDK call is inside the store: the host asks again
        }
        let r = (e.h)(seq, buf, body, blen);
        sw::unlock();
        return r;
    }
    // a command a module registered
    match service(cmd) {
        Some(h) => {
            let out = (addr_of_mut!(RBODY) as *mut u8).add(1);
            // what the handler logs carries its module's source (owner 255: registered by no module)
            let owner = service_of(cmd).map_or(255, |s| s.owner);
            let prev = if owner != 255 && owner > 0 { dlog::enter_module(owner as usize - 1, dlog::PHASE_CALLBACK) } else { 0 };
            let r = h(cmd as u32, seq, body, blen as u32, out);
            if owner != 255 && owner > 0 {
                dlog::leave_module(prev);
            }
            if r < 0 {
                reply(cmd, seq, r, 0)
            } else if r as usize > REPLY_BODY_MAX - 1 {
                reply(cmd, seq, E_LENGTH, 0)
            } else {
                reply(cmd, seq, OK, r as usize)
            }
        }
        None => reply(cmd, seq, E_PROTO, 0),
    }
}

// ---- the command table ---------------------------------------------------------------------------------------------------

/// A built-in command: its handler takes the request number, the buffer (table scratch), the body and its length, and sends the reply.
pub(crate) type Handler = unsafe fn(u32, *mut u8, *const u8, usize) -> i32;

/// The reply of this command is not logged as an UPLOAD event (it is read-only and frequent).
pub(crate) const NOLOG: u8 = 1;
/// The body is read in place (`buf + BODY_AT`, up to a chunk), not copied into the small request buffer.
pub(crate) const BIG: u8 = 2;
/// The command reads or writes the table or the card through the store: it holds the store lock.
pub(crate) const STORE: u8 = 4;

pub(crate) struct Cmd {
    cmd: u8,
    flags: u8,
    h: Handler,
}

/// The built-in commands, looked up one after the other (a jump table is not ColdFire code).
pub(crate) static CMDS: [Cmd; 19] = [
    Cmd { cmd: CMD_INFO, flags: NOLOG | STORE, h: h_info },
    Cmd { cmd: CMD_LIST, flags: STORE, h: h_list },
    Cmd { cmd: CMD_MODULES, flags: NOLOG, h: h_modules },
    Cmd { cmd: CMD_CAPACITY, flags: NOLOG, h: h_capacity },
    Cmd { cmd: CMD_LOADS, flags: NOLOG, h: h_loads },
    Cmd { cmd: CMD_BEGIN, flags: STORE, h: h_begin },
    Cmd { cmd: CMD_DATA, flags: BIG | STORE, h: h_data },
    Cmd { cmd: CMD_END, flags: STORE, h: h_end },
    Cmd { cmd: CMD_DELETE, flags: STORE, h: h_delete },
    Cmd { cmd: CMD_MKDIR, flags: STORE, h: h_mkdir },
    Cmd { cmd: CMD_RENAME, flags: STORE, h: h_rename },
    Cmd { cmd: CMD_READ, flags: NOLOG | STORE, h: h_read },
    Cmd { cmd: CMD_VERIFY, flags: NOLOG | STORE, h: h_verify },
    Cmd { cmd: CMD_SETTINGS, flags: NOLOG, h: h_settings },
    Cmd { cmd: CMD_CLEAR, flags: STORE, h: h_clear },
    Cmd { cmd: CMD_ABORT, flags: 0, h: h_abort },
    Cmd { cmd: CMD_FAULT, flags: 0, h: h_fault },
    Cmd { cmd: CMD_SELECT, flags: BIG, h: h_select },
    Cmd { cmd: CMD_RESTART, flags: 0, h: h_restart },
];

pub(crate) fn find(cmd: u8) -> Option<&'static Cmd> {
    let mut i = 0;
    while i < CMDS.len() {
        if CMDS[i].cmd == cmd {
            return Some(&CMDS[i]);
        }
        i += 1;
    }
    None
}

/// Is the reply to `cmd` with `status` logged as an UPLOAD event? Not a read-only built-in (flag), and a service command only when it failed
/// (a key press must not fill the ring).
pub(crate) fn logged(cmd: u8, status: i32) -> bool {
    match find(cmd) {
        Some(e) => e.flags & NOLOG == 0,
        None => status != 0 || service(cmd).is_none(),
    }
}

